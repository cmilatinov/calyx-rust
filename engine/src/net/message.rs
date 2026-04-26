use crate::core::TimeType;
use crate::net::NetworkObjectId;
use renet::{ClientId, DefaultChannel};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameChannel {
    Unreliable,
    ReliableUnordered,
    ReliableOrdered,
}

impl GameChannel {
    pub const ALL: [Self; 3] = [
        Self::Unreliable,
        Self::ReliableUnordered,
        Self::ReliableOrdered,
    ];
}

impl From<GameChannel> for u8 {
    fn from(channel: GameChannel) -> Self {
        match channel {
            GameChannel::Unreliable => DefaultChannel::Unreliable.into(),
            GameChannel::ReliableUnordered => DefaultChannel::ReliableUnordered.into(),
            GameChannel::ReliableOrdered => DefaultChannel::ReliableOrdered.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerEvent {
    ClientConnected { client_id: ClientId },
    ClientDisconnected { client_id: ClientId },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GameMessage {
    // SERVER EVENTS
    ServerEvent(ServerEvent),
    // CLIENT EVENTS
    SelfConnected {
        client_ids: Vec<ClientId>,
    },
    ClientConnected {
        client_id: ClientId,
    },
    ClientDisconnected {
        client_id: ClientId,
    },
    // SYNCHRONIZATION EVENTS
    SyncTime {
        current_time: TimeType,
    },
    SyncComponent {
        time: TimeType,
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        component_uuid: Uuid,
        data: Vec<u8>,
    },
    TransferOwnership {
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        to_client_id: ClientId,
    },
    SpawnPrefab {
        network_object_ids: HashMap<u32, NetworkObjectId>,
        from_client_id: ClientId,
        prefab_id: Uuid,
    },
    DestroyGameObject {
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
    },
}

impl GameMessage {
    const PROTOCOL_SCHEMA: &str = concat!(
        "ServerEvent{ClientConnected(client_id:ClientId),",
        "ClientDisconnected(client_id:ClientId)};",
        "GameMessage{ServerEvent(ServerEvent),",
        "SelfConnected(client_ids:Vec<ClientId>),",
        "ClientConnected(client_id:ClientId),",
        "ClientDisconnected(client_id:ClientId),",
        "SyncTime(current_time:TimeType),",
        "SyncComponent(time:TimeType,network_object_id:NetworkObjectId,",
        "from_client_id:ClientId,component_uuid:Uuid,data:Vec<u8>),",
        "TransferOwnership(network_object_id:NetworkObjectId,",
        "from_client_id:ClientId,to_client_id:ClientId),",
        "SpawnPrefab(network_object_ids:HashMap<u32,NetworkObjectId>,",
        "from_client_id:ClientId,prefab_id:Uuid),",
        "DestroyGameObject(network_object_id:NetworkObjectId,from_client_id:ClientId)}",
    );

    pub const PROTOCOL_ID: u64 = fnv1a64(Self::PROTOCOL_SCHEMA.as_bytes());

    pub fn channel(&self) -> GameChannel {
        match self {
            Self::SyncTime { .. } | Self::SyncComponent { .. } => GameChannel::Unreliable,
            Self::SelfConnected { .. } => GameChannel::ReliableUnordered,
            Self::ServerEvent(_)
            | Self::ClientConnected { .. }
            | Self::ClientDisconnected { .. }
            | Self::TransferOwnership { .. }
            | Self::SpawnPrefab { .. }
            | Self::DestroyGameObject { .. } => GameChannel::ReliableOrdered,
        }
    }
}

const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        index += 1;
    }
    hash
}

#[derive(Debug)]
pub struct MessageQueue<T> {
    messages: VecDeque<T>,
}

impl Default for MessageQueue<GameMessage> {
    fn default() -> Self {
        Self {
            messages: Default::default(),
        }
    }
}

pub trait MessageHandler<C, M> {
    fn handle_message(&mut self, context: &mut C, message: &M) -> MessageHandlerResult;
}

pub enum MessageHandlerResult {
    Consume,
    Ignore,
}

impl<T> MessageQueue<T> {
    pub fn queue_message(&mut self, message: T) {
        self.messages.push_back(message);
    }

    pub fn receive_messages<C, H: MessageHandler<C, T>>(
        &mut self,
        context: &mut C,
        handler: &mut H,
    ) {
        self.messages.retain(|msg| {
            matches!(
                handler.handle_message(context, msg),
                MessageHandlerResult::Ignore
            )
        });
    }
}
