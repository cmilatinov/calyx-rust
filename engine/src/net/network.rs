use crate as engine;
use crate::context::ReadOnlyAssetContext;
use crate::core::{Ref, Time, TimeType};
use crate::error::BoxedError;
use crate::net::client::Client;
use crate::net::server::Server;
use crate::net::{
    ComponentNetworkObject, GameMessage, MessageHandler, MessageHandlerResult, MessageQueue,
    NetworkObjectId, ServerEvent,
};
use crate::scene::{GameObject, Prefab, Scene};
use crate::try_all;
use engine_derive::Resource;
use log::trace;
use rand::Rng;
use renet::{ClientId, DefaultChannel};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

#[derive(Resource)]
pub struct Network {
    pub client: Client,
    pub server: Option<Server>,
    pub queue: MessageQueue<GameMessage>,
    pub local_id: Option<ClientId>,
    tick_rate: TimeType,
    tick_period: TimeType,
    accumulated_time: TimeType,
}

impl Default for Network {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

impl Network {
    const DEFAULT_TICK_RATE_HZ: f32 = 30.0;

    fn new_id() -> NetworkObjectId {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let rand_part: u128 = rand::rng().random();
        let combined = now ^ rand_part;
        (combined & 0xFFFF_FFFF) as u32
    }

    pub fn new(tick_rate: f32) -> Self {
        assert!(tick_rate > 0.0);
        Self {
            client: Default::default(),
            server: None,
            queue: Default::default(),
            local_id: None,
            tick_rate,
            tick_period: 1.0 / tick_rate,
            accumulated_time: 0.0,
        }
    }

    pub fn host(&mut self, socket_addr: SocketAddr) -> Result<(), BoxedError> {
        self.server = Some(Server::new(socket_addr)?);
        self.local_id = Some(Self::new_id() as u64);
        Ok(())
    }

    pub fn update(&mut self, time: &mut Time) {
        self.accumulated_time += time.static_duration().as_secs_f32();
        while self.accumulated_time >= self.tick_period {
            self.accumulated_time -= self.tick_period;
            let duration = Duration::from_secs_f32(self.tick_period);
            if let Some(server) = &mut self.server {
                server.update(&mut self.queue, duration);
                if time.timer("NETWORK_TICK_SYNC") >= 1.0 {
                    let _ = server.broadcast_message_except(
                        0,
                        DefaultChannel::ReliableOrdered,
                        &GameMessage::SyncTime {
                            current_time: time.time,
                        },
                    );
                    time.reset_timer("NETWORK_TICK_SYNC");
                }
            }
            self.client.update(&mut self.queue, duration);
        }
        self.queue
            .receive_messages(&mut (&mut time.time, self.tick_rate), &mut NetworkTimeSync);
    }

    pub fn update_scene<'a>(
        &'a mut self,
        mut scene: &'a mut Scene,
        assets: &'a ReadOnlyAssetContext,
    ) {
        self.queue.receive_messages(
            &mut (&mut self.client, &mut self.server, &mut scene, &mut self.local_id),
            &mut NetworkSceneSync,
        );
        self.queue.receive_messages(
            &mut (&mut self.client, &mut self.server, &mut scene, assets, &mut self.local_id),
            &mut NetworkPrefabSync,
        );
    }

    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }

    pub fn tick_period(&self) -> TimeType {
        self.tick_period
    }

    pub fn tick_rate(&self) -> TimeType {
        self.tick_rate
    }

    fn traverse_prefab(
        client_id: ClientId,
        scene: &mut Scene,
        root: GameObject,
        index: &mut u32,
        src_ids: Option<&HashMap<u32, NetworkObjectId>>,
        dst_ids: &mut Option<&mut HashMap<u32, NetworkObjectId>>,
    ) {
        'traverse: {
            try_all!(
                None => break 'traverse;
                let mut entry = scene.entry_mut(root);
                let c_netobj = entry.get_component_mut::<ComponentNetworkObject>().ok();
            );
            c_netobj.owner_id = client_id;
            c_netobj.id = src_ids
                .and_then(|ids| ids.get(index).copied())
                .unwrap_or_else(|| Self::new_id());
            if let Some(dst_ids) = dst_ids {
                dst_ids.insert(*index, c_netobj.id);
            }
        }
        *index += 1;
        for child in scene.children_ordered(root).collect::<Vec<_>>() {
            Self::traverse_prefab(client_id, scene, child, index, src_ids, dst_ids);
        }
    }

    pub fn instantiate_prefab(
        &mut self,
        scene: &mut Scene,
        prefab_ref: Ref<Prefab>,
    ) -> Option<GameObject> {
        let client_id = self.local_id?;
        let prefab = prefab_ref.read();
        let prefab_root = scene.instantiate_prefab(&prefab, None)?;
        let mut index = 0;
        let mut network_object_ids = Default::default();
        Self::traverse_prefab(
            client_id,
            scene,
            prefab_root,
            &mut index,
            None,
            &mut Some(&mut network_object_ids),
        );
        let _ = self.client.send_message(&GameMessage::SpawnPrefab {
            network_object_ids,
            from_client_id: client_id,
            prefab_id: prefab_ref.id(),
        });
        Some(prefab_root)
    }
}

struct NetworkTimeSync;
impl MessageHandler<(&mut TimeType, TimeType), GameMessage> for NetworkTimeSync {
    fn handle_message(
        &mut self,
        (time_counter, tick_rate): &mut (&mut TimeType, TimeType),
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SyncTime { current_time } => {
                **time_counter = *current_time + (1.0 / *tick_rate);
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

pub struct NetworkSceneSync;
type NetworkSceneSyncContext<'a, 'b> = (
    &'a mut Client,
    &'a mut Option<Server>,
    &'a mut &'b mut Scene,
    &'a mut Option<ClientId>,
);

impl MessageHandler<NetworkSceneSyncContext<'_, '_>, GameMessage> for NetworkSceneSync {
    fn handle_message(
        &mut self,
        (client, server, scene, local_id): &mut NetworkSceneSyncContext,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::ServerEvent(ServerEvent::ClientConnected { client_id }) => {
                try_all!(
                    None => return MessageHandlerResult::Consume;
                    let server = server;
                );
                let _ = server.send_message(
                    *client_id,
                    DefaultChannel::ReliableOrdered,
                    &GameMessage::SelfConnected {
                        client_ids: server.client_ids(),
                    },
                );
                let _ = server.broadcast_message_except(
                    *client_id,
                    DefaultChannel::ReliableOrdered,
                    &GameMessage::ClientConnected {
                        client_id: *client_id,
                    },
                );
                MessageHandlerResult::Consume
            }
            GameMessage::SelfConnected {
                client_ids,
            } => {
                #[allow(unused)]
                'client_logic: {
                    let self_client_id = client.client_id();
                    **local_id = self_client_id;
                    client.client_ids = client_ids.clone();
                    client.client_ids.retain(|cid| self_client_id != Some(*cid));
                    trace!("Self connected, local_id: {:?}", self_client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientConnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.push(*client_id);
                    trace!("Client Connected: {:?}", client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientDisconnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.retain(|&id| id != *client_id);
                    trace!("Client Disconnected: {:?}", client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::TransferOwnership {
                network_object_id,
                from_client_id,
                to_client_id,
            } => {
                // TODO(Cristian): Optimize this, maybe cache network ids in scene
                let Some(game_object) = scene.objects().find(|go| {
                    let Some(entry) = scene.entry(*go) else {
                        return false;
                    };
                    let Ok(c_netobj) = entry.get_component::<ComponentNetworkObject>() else {
                        return false;
                    };
                    c_netobj.id == *network_object_id && c_netobj.owner_id == *from_client_id
                }) else {
                    return MessageHandlerResult::Consume;
                };
                'server_logic: {
                    try_all!(
                        None => break 'server_logic;
                        let server = server;
                    );
                    trace!("SERVER - Transferring ownership");
                    let _ = server.broadcast_message(DefaultChannel::ReliableOrdered, message);
                }
                'client_logic: {
                    try_all!(
                        None => break 'client_logic;
                        let mut entry = scene.entry_mut(game_object);
                        let c_netobj = entry.get_component_mut::<ComponentNetworkObject>().ok();
                    );
                    trace!("CLIENT - Transferring ownership");
                    c_netobj.owner_id = *to_client_id;
                }
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

struct NetworkPrefabSync;
type NetworkPrefabSyncContext<'a, 'b> = (
    &'a mut Client,
    &'a mut Option<Server>,
    &'a mut &'b mut Scene,
    &'a ReadOnlyAssetContext,
    &'a mut Option<ClientId>,
);
impl MessageHandler<NetworkPrefabSyncContext<'_, '_>, GameMessage> for NetworkPrefabSync {
    fn handle_message(
        &mut self,
        (_client, server, scene, context, local_id): &mut NetworkPrefabSyncContext,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SpawnPrefab {
                from_client_id,
                network_object_ids,
                prefab_id,
            } => {
                if let Some(server) = server {
                    let _ = server.broadcast_message_except(
                        *from_client_id,
                        DefaultChannel::ReliableOrdered,
                        message,
                    );
                    return MessageHandlerResult::Consume;
                }
                try_all!(
                    None => return MessageHandlerResult::Consume;
                    let client_id = **local_id;
                );
                if client_id == *from_client_id {
                    return MessageHandlerResult::Consume;
                }
                try_all!(
                    None => return MessageHandlerResult::Consume;
                    let prefab_ref = context.registries.assets
                        .read()
                        .load_by_id::<Prefab>(*prefab_id)
                        .ok();
                );
                let prefab = prefab_ref.read();
                let Some(prefab_root) = scene.instantiate_prefab(&prefab, None) else {
                    return MessageHandlerResult::Consume;
                };
                let mut index = 0;
                Network::traverse_prefab(
                    client_id,
                    scene,
                    prefab_root,
                    &mut index,
                    Some(network_object_ids),
                    &mut None,
                );
                MessageHandlerResult::Consume
            }
            GameMessage::DestroyGameObject { .. } => MessageHandlerResult::Consume,
            _ => MessageHandlerResult::Ignore,
        }
    }
}
