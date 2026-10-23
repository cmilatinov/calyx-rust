use crate as engine;
use crate::context::ReadOnlyAssetContext;
use crate::core::{AbsoluteTime, Ref, Time, TimeType};
use crate::error::BoxedError;
use crate::net::client::Client;
use crate::net::server::Server;
use crate::net::{
    ComponentNetworkObject, GameMessage, MessageHandler, MessageHandlerResult, MessageQueue,
    MessageSender, NetworkObjectId, ServerEvent,
};
use crate::scene::{GameObject, Prefab, Scene};
use crate::utils::TypeUuid;
use engine_derive::Resource;
use renet::ClientId;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

/// High-level networking resource that owns the active client, optional host
/// server, protocol queue, and replication tick state.
#[derive(Resource, TypeUuid)]
#[uuid = "657869ba-a509-4340-89ae-f07d1eddf0db"]
#[repr(C)]
pub struct Network {
    /// Local client connection and transport state.
    pub client: Client,
    /// Local host server when running in listen-server mode.
    pub server: Option<Server>,
    /// Shared queue of decoded protocol messages.
    pub queue: MessageQueue<GameMessage>,
    /// Client ID assigned to the local peer once connected or hosting.
    pub local_id: Option<ClientId>,
    tick_rate: TimeType,
    tick_period: TimeType,
    accumulated_time: TimeType,
    /// Seconds of unscaled time since the last time-sync broadcast.
    tick_sync_timer: TimeType,
}

impl Default for Network {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

/// Randomized process prefix plus a monotonic counter.
/// This keeps IDs ordered within a process while avoiding collisions across peers.
static NETWORK_ID_PREFIX: LazyLock<NetworkObjectId> =
    LazyLock::new(|| u64::from(rand::random::<u32>()) << 32);
static NEXT_NETWORK_ID: AtomicU32 = AtomicU32::new(1);

impl Network {
    const DEFAULT_TICK_RATE_HZ: f32 = 30.0;

    pub(crate) fn new_id() -> NetworkObjectId {
        *NETWORK_ID_PREFIX | u64::from(NEXT_NETWORK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Creates a network resource with a fixed replication tick rate.
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
            tick_sync_timer: 0.0,
        }
    }

    /// Starts a local host server and assigns a local client ID for the host
    /// peer.
    pub fn host(&mut self, socket_addr: SocketAddr) -> Result<(), BoxedError> {
        self.server = Some(Server::new(socket_addr)?);
        self.local_id = Some(Client::generate_client_id());
        Ok(())
    }

    /// Advances the networking tick loop and dispatches global protocol
    /// messages such as time sync.
    ///
    /// A tick runs from one call to the next: this call, then
    /// [`Network::update_scene`], then the synchronized component updates.
    /// Every handler has had its pass on a message by the next call, so this
    /// call first drops whatever is still queued, and any component sync the
    /// host did not relay, before polling new messages.
    pub fn update(&mut self, time: &mut Time) {
        let expired = self.queue.expire();
        if expired > 0 {
            log::trace!("Expired {expired} network messages no handler consumed");
        }
        if let Some(server) = &mut self.server {
            server.discard_component_syncs();
        }
        self.accumulated_time += time.static_duration().as_secs_f32();
        self.tick_sync_timer += time.static_delta_time();
        while self.accumulated_time >= self.tick_period {
            self.accumulated_time -= self.tick_period;
            let duration = Duration::from_secs_f32(self.tick_period);
            if let Some(server) = &mut self.server {
                server.update(&mut self.queue, duration);
                if self.tick_sync_timer >= 1.0 {
                    if let Err(e) = server.broadcast_message_except(
                        0,
                        &GameMessage::SyncTime {
                            current_time: time.time(),
                        },
                    ) {
                        log::warn!("Failed to broadcast time sync: {e}");
                    }
                    self.tick_sync_timer = 0.0;
                }
            }
            self.client.update(&mut self.queue, duration);
        }
        self.queue
            .receive_messages(None, &mut (time, self.tick_rate), &mut NetworkTimeSync);
    }

    /// Applies queued scene-related replication messages.
    ///
    /// This consumes ownership, connection, prefab-spawn and destroy messages
    /// against `scene`, then relays the clients' component syncs on the host.
    /// Call this after [`Network::update`] in the main simulation loop.
    pub fn update_scene<'a>(
        &'a mut self,
        mut scene: &'a mut Scene,
        assets: &'a ReadOnlyAssetContext,
    ) {
        // One pass over the queue, so lifecycle messages for the same object
        // (spawn, ownership transfer, destroy) apply in the order they were
        // sent.
        self.queue.receive_messages(
            None,
            &mut (
                &mut self.client,
                &mut self.server,
                &mut scene,
                assets,
                &mut self.local_id,
            ),
            &mut NetworkReplicationSync,
        );
        // After the pass, so a spawn received with its first syncs is known.
        if let Some(server) = &mut self.server {
            server.relay_component_syncs(|network_object_id| {
                find_network_object(scene, network_object_id).map(|(_, owner_id)| owner_id)
            });
        }
    }

    /// Destroys `game_object` locally and tells the other peers to destroy
    /// their copy.
    ///
    /// Only the owner of a network object may destroy it. Returns `false`,
    /// leaving the object in place, when the local peer has no id yet, the
    /// object has no [`ComponentNetworkObject`], or another peer owns it.
    pub fn destroy_game_object(&mut self, scene: &mut Scene, game_object: GameObject) -> bool {
        let Some(client_id) = self.local_id else {
            return false;
        };
        let Some((network_object_id, owner_id)) = scene
            .read_component::<ComponentNetworkObject, _, _>(game_object, |c| (c.id, c.owner_id))
        else {
            return false;
        };
        if owner_id != client_id {
            log::warn!(
                "Refusing to destroy network object {network_object_id} owned by {owner_id}"
            );
            return false;
        }
        scene.delete(game_object);
        let message = GameMessage::DestroyGameObject {
            network_object_id,
            from_client_id: client_id,
        };
        let result = match &mut self.server {
            Some(server) => server.broadcast_message(&message),
            None => self.client.send_message(&message),
        };
        if let Err(e) = result {
            log::warn!("Failed to send DestroyGameObject: {e}");
        }
        true
    }

    /// Returns whether this network resource is hosting a local server.
    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }

    /// Returns the fixed tick period in seconds.
    pub fn tick_period(&self) -> TimeType {
        self.tick_period
    }

    /// Returns the fixed tick rate in hertz.
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
        // Written through the scene so its network object index sees the id.
        let mut network_object_id = None;
        scene.write_component::<ComponentNetworkObject, _>(root, |c_netobj| {
            c_netobj.owner_id = client_id;
            c_netobj.id = src_ids
                .and_then(|ids| ids.get(index).copied())
                .unwrap_or_else(Self::new_id);
            network_object_id = Some(c_netobj.id);
        });
        if let (Some(network_object_id), Some(dst_ids)) = (network_object_id, dst_ids.as_mut()) {
            dst_ids.insert(*index, network_object_id);
        }
        *index += 1;
        for child in scene.children_ordered(root).collect::<Vec<_>>() {
            Self::traverse_prefab(client_id, scene, child, index, src_ids, dst_ids);
        }
    }

    /// Instantiates `prefab_ref` locally, assigns network object IDs, and
    /// broadcasts the spawn to other peers.
    pub fn instantiate_prefab(
        &mut self,
        scene: &mut Scene,
        prefab_ref: Ref<Prefab>,
    ) -> Option<GameObject> {
        let client_id = self.local_id?;
        let prefab = prefab_ref.read();
        let prefab_root = match scene.instantiate_prefab(&prefab, None) {
            Ok(prefab_root) => prefab_root,
            Err(err) => {
                log::warn!("Failed to instantiate prefab {}: {err}", prefab_ref.id());
                return None;
            }
        };
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
        let message = GameMessage::SpawnPrefab {
            network_object_ids,
            from_client_id: client_id,
            prefab_id: prefab_ref.id(),
        };
        // A listen-server host has no connection of its own to send through.
        let result = match &mut self.server {
            Some(server) => server.broadcast_message(&message),
            None => self.client.send_message(&message),
        };
        if let Err(e) = result {
            log::warn!("Failed to send SpawnPrefab: {e}");
        }
        Some(prefab_root)
    }
}

struct NetworkTimeSync;
impl MessageHandler<(&mut Time, TimeType), GameMessage> for NetworkTimeSync {
    fn handle_message(
        &mut self,
        (time, tick_rate): &mut (&mut Time, TimeType),
        _sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SyncTime { current_time } => {
                time.set_time(*current_time + (1.0 / AbsoluteTime::from(*tick_rate)));
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

/// Message handler that applies connection lifecycle and ownership-transfer
/// messages to scene and client state.
///
/// Most callers should use [`Network::update_scene`] instead of invoking this
/// handler directly.
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
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::ServerEvent(ServerEvent::ClientConnected { client_id }) => {
                let Some(server) = server else {
                    return MessageHandlerResult::Consume;
                };
                if let Err(e) = server.send_message(
                    *client_id,
                    &GameMessage::SelfConnected {
                        client_ids: server.client_ids(),
                    },
                ) {
                    log::warn!("Failed to send SelfConnected to {client_id}: {e}");
                }
                if let Err(e) = server.broadcast_message_except(
                    *client_id,
                    &GameMessage::ClientConnected {
                        client_id: *client_id,
                    },
                ) {
                    log::warn!("Failed to broadcast ClientConnected: {e}");
                }
                MessageHandlerResult::Consume
            }
            GameMessage::SelfConnected { client_ids } => {
                #[allow(unused)]
                'client_logic: {
                    let self_client_id = client.client_id();
                    **local_id = self_client_id;
                    client.client_ids = client_ids.clone();
                    client.client_ids.retain(|cid| self_client_id != Some(*cid));
                    log::trace!("Self connected, local_id: {self_client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientConnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.push(*client_id);
                    log::trace!("Client Connected: {client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientDisconnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.retain(|&id| id != *client_id);
                    log::trace!("Client Disconnected: {client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::TransferOwnership {
                network_object_id,
                from_client_id,
                to_client_id,
            } => {
                let acting_client_id = sender.acting_client(*from_client_id);
                let Some(game_object) = find_network_object(scene, *network_object_id)
                    .filter(|(_, owner_id)| *owner_id == acting_client_id)
                    .map(|(game_object, _)| game_object)
                else {
                    return MessageHandlerResult::Consume;
                };
                'server_logic: {
                    let Some(server) = server else {
                        break 'server_logic;
                    };
                    log::trace!("SERVER - Transferring ownership");
                    if let Err(e) = server.broadcast_message(message) {
                        log::warn!("Failed to broadcast TransferOwnership: {e}");
                    }
                }
                let transferred =
                    scene.write_component::<ComponentNetworkObject, _>(game_object, |c_netobj| {
                        c_netobj.owner_id = *to_client_id;
                    });
                if transferred.is_some() {
                    log::trace!("CLIENT - Transferring ownership");
                }
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

/// Applies a `DestroyGameObject` request delivered by `sender`.
///
/// A request from a client that does not own the object is dropped. On the
/// server, an accepted request is relayed to every other client; the server
/// may not hold a copy of every object, so it relays requests for objects it
/// does not know and each receiving peer checks ownership itself.
pub(crate) fn handle_destroy(
    server: &mut Option<Server>,
    scene: &mut Scene,
    sender: MessageSender,
    network_object_id: NetworkObjectId,
    from_client_id: ClientId,
    message: &GameMessage,
) {
    let acting_client_id = sender.acting_client(from_client_id);
    let target = find_network_object(scene, network_object_id);
    if let Some((_, owner_id)) = target {
        if owner_id != acting_client_id {
            log::warn!(
                "Rejected destroy of network object {network_object_id} owned by {owner_id} \
                 from {acting_client_id}"
            );
            return;
        }
    }
    if let Some(server) = server {
        if let Err(e) = server.broadcast_message_except(acting_client_id, message) {
            log::warn!("Failed to broadcast DestroyGameObject: {e}");
        }
    }
    if let Some((game_object, _)) = target {
        scene.delete(game_object);
    }
}

/// Finds the game object carrying network object `network_object_id` and
/// returns it with its owner.
fn find_network_object(
    scene: &mut Scene,
    network_object_id: NetworkObjectId,
) -> Option<(GameObject, ClientId)> {
    let game_object = scene.find_network_object(network_object_id)?;
    scene
        .read_component::<ComponentNetworkObject, _, _>(game_object, |c| c.owner_id)
        .map(|owner_id| (game_object, owner_id))
}

/// Message handler that offers each message to [`NetworkSceneSync`], then to
/// [`NetworkPrefabSync`], so every scene replication message is handled in
/// one FIFO pass over the queue.
struct NetworkReplicationSync;
impl MessageHandler<NetworkPrefabSyncContext<'_, '_>, GameMessage> for NetworkReplicationSync {
    fn handle_message(
        &mut self,
        context: &mut NetworkPrefabSyncContext,
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        let (client, server, scene, assets, local_id) = context;
        let result = NetworkSceneSync.handle_message(
            &mut (&mut **client, &mut **server, &mut **scene, &mut **local_id),
            sender,
            message,
        );
        match result {
            MessageHandlerResult::Consume => MessageHandlerResult::Consume,
            MessageHandlerResult::Ignore => NetworkPrefabSync.handle_message(
                &mut (client, server, scene, assets, local_id),
                sender,
                message,
            ),
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
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SpawnPrefab {
                from_client_id,
                network_object_ids,
                prefab_id,
            } => {
                let acting_client_id = sender.acting_client(*from_client_id);
                // The server relays the spawn to every other client, then
                // instantiates it like any other peer.
                if let Some(server) = server {
                    if let Err(e) = server.broadcast_message_except(acting_client_id, message) {
                        log::warn!("Failed to broadcast SpawnPrefab: {e}");
                    }
                }
                let Some(client_id) = **local_id else {
                    return MessageHandlerResult::Consume;
                };
                if client_id == acting_client_id {
                    return MessageHandlerResult::Consume;
                }
                let Some(prefab_ref) = context
                    .registries
                    .assets
                    .read()
                    .load_by_id::<Prefab>(*prefab_id)
                    .ok()
                else {
                    return MessageHandlerResult::Consume;
                };
                let prefab = prefab_ref.read();
                let prefab_root = match scene.instantiate_prefab(&prefab, None) {
                    Ok(prefab_root) => prefab_root,
                    Err(err) => {
                        log::warn!("Failed to instantiate spawned prefab {prefab_id}: {err}");
                        return MessageHandlerResult::Consume;
                    }
                };
                // The spawning peer owns the replicated objects on every peer.
                let mut index = 0;
                Network::traverse_prefab(
                    acting_client_id,
                    scene,
                    prefab_root,
                    &mut index,
                    Some(network_object_ids),
                    &mut None,
                );
                MessageHandlerResult::Consume
            }
            // Handled in the same pass as `SpawnPrefab`, so a destroy that
            // follows a spawn is applied and relayed after it.
            GameMessage::DestroyGameObject {
                network_object_id,
                from_client_id,
            } => {
                handle_destroy(
                    server,
                    scene,
                    sender,
                    *network_object_id,
                    *from_client_id,
                    message,
                );
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}
