use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::net::{
    Client, GameMessage, MessageHandler, MessageHandlerResult, Network, Server, ServerEvent,
};
use crate::reflect::{Reflect, ReflectDefault};
use crate::scene::Scene;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::IntoQuery;
use renet::{ClientId, DefaultChannel};
use serde::{Deserialize, Serialize};

pub struct NetworkSceneSync;
type NetworkSceneSyncContext<'a> = (&'a mut Scene, &'a mut Client, &'a mut Option<Server>);

impl MessageHandler<NetworkSceneSyncContext<'_>, GameMessage> for NetworkSceneSync {
    fn handle_message(
        &mut self,
        (scene, client, server): &mut NetworkSceneSyncContext,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::ServerEvent(ServerEvent::ClientConnected { client_id }) => {
                'server_logic: {
                    let Some(server) = server else {
                        break 'server_logic;
                    };
                    let _ = server.send_message(
                        *client_id,
                        DefaultChannel::ReliableOrdered,
                        &GameMessage::SelfConnected {
                            server_client_id: client.client_id().unwrap(),
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
                }
                MessageHandlerResult::Consume
            }
            GameMessage::SelfConnected {
                server_client_id,
                client_ids,
            } => {
                #[allow(unused)]
                'client_logic: {
                    let mut query = <&mut ComponentNetworkObject>::query();
                    for c_netobj in query.iter_mut(&mut scene.world) {
                        c_netobj.owner_id = *server_client_id;
                    }
                    let self_client_id = client.client_id();
                    client.client_ids = client_ids.clone();
                    client.client_ids.retain(|cid| self_client_id != Some(*cid));
                    println!("Server Client ID: {:?}", server_client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientConnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.push(*client_id);
                    println!("Client Connected: {:?}", client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientDisconnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.retain(|&id| id != *client_id);
                    println!("Client Disconnected: {:?}", client_id);
                };
                MessageHandlerResult::Consume
            }
            GameMessage::TransferOwnership {
                network_object_id,
                from_client_id,
                to_client_id,
            } => {
                // TODO(Cristian): Optimize this, maybe cache network ids in scene
                let Some(game_object) = scene.game_objects().find(|go| {
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
                    let Some(server) = server else {
                        break 'server_logic;
                    };
                    println!("SERVER - Transferring ownership");
                    let _ = server.broadcast_message(DefaultChannel::ReliableOrdered, message);
                }
                'client_logic: {
                    let Some(mut entry) = scene.entry_mut(game_object) else {
                        break 'client_logic;
                    };
                    let Ok(c_netobj) = entry.get_component_mut::<ComponentNetworkObject>() else {
                        break 'client_logic;
                    };
                    println!("CLIENT - Transferring ownership");
                    c_netobj.owner_id = *to_client_id;
                }
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

pub type NetworkObjectId = u32;

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "adca3c70-5d3d-4b32-83cc-7bdf04a8358a"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Object")]
#[serde(default)]
#[repr(C)]
pub struct ComponentNetworkObject {
    pub id: NetworkObjectId,
    pub owner_id: ClientId,
}

impl Component for ComponentNetworkObject {}

impl ComponentNetworkObject {
    pub fn is_owner(&self, network: &Network) -> bool {
        network.client.client_id() == Some(self.owner_id)
    }
}
