use crate::core::TimeType;
use crate::net::NetworkObjectId;
use renet::{ClientId, DefaultChannel};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

/// Logical Renet channels used by the engine's multiplayer protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameChannel {
    /// Unreliable channel for time-sensitive state that can be dropped.
    Unreliable,
    /// Reliable channel with no ordering guarantee between messages.
    ReliableUnordered,
    /// Reliable ordered channel for gameplay and lifecycle events.
    ReliableOrdered,
}

impl GameChannel {
    /// All protocol channels in the order polled by client and server updates.
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

/// Connection lifecycle events emitted by the server and fed back through the
/// engine message queue.
#[derive(Debug, Serialize, Deserialize)]
pub enum ServerEvent {
    /// A client joined the session.
    ClientConnected { client_id: ClientId },
    /// A client left the session.
    ClientDisconnected { client_id: ClientId },
}

/// Messages exchanged between clients and the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum GameMessage {
    // SERVER EVENTS
    /// Wrapped server lifecycle event.
    ServerEvent(ServerEvent),
    // CLIENT EVENTS
    /// Sent to a newly connected client with its peer list.
    SelfConnected { client_ids: Vec<ClientId> },
    /// Broadcast when a new peer joins.
    ClientConnected { client_id: ClientId },
    /// Broadcast when a peer disconnects.
    ClientDisconnected { client_id: ClientId },
    // SYNCHRONIZATION EVENTS
    /// Periodic host-to-client clock synchronization message.
    SyncTime { current_time: TimeType },
    /// Replicated component state for one network object.
    SyncComponent {
        time: TimeType,
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        component_uuid: Uuid,
        data: Vec<u8>,
    },
    /// Ownership transfer for a replicated object.
    TransferOwnership {
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        to_client_id: ClientId,
    },
    /// Prefab spawn request or replication event.
    SpawnPrefab {
        network_object_ids: HashMap<u32, NetworkObjectId>,
        from_client_id: ClientId,
        prefab_id: Uuid,
    },
    /// Request to destroy a replicated object.
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

    /// Stable FNV-1a hash of the protocol schema string.
    ///
    /// Both client and server use this to reject incompatible builds.
    pub const PROTOCOL_ID: u64 = fnv1a64(Self::PROTOCOL_SCHEMA.as_bytes());

    /// Returns the transport channel to use for this message type.
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

/// FIFO queue of decoded network messages waiting to be handled by engine
/// systems.
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

/// Callback interface used to consume queued network messages.
pub trait MessageHandler<C, M> {
    fn handle_message(&mut self, context: &mut C, message: &M) -> MessageHandlerResult;
}

/// Outcome of handling one queued message.
pub enum MessageHandlerResult {
    /// Message was handled and should be removed from the queue.
    Consume,
    /// Message was not handled and should remain queued for another handler.
    Ignore,
}

impl<T> MessageQueue<T> {
    /// Appends a decoded message to the queue.
    pub fn queue_message(&mut self, message: T) {
        self.messages.push_back(message);
    }

    /// Dispatches queued messages to `handler`.
    ///
    /// Messages that return [`MessageHandlerResult::Consume`] are removed.
    /// Messages that return [`MessageHandlerResult::Ignore`] stay queued for a
    /// later pass.
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
