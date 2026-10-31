use crate::core::AbsoluteTime;
use crate::net::{NetworkIdRange, NetworkObjectId};
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
    /// Sent to a newly connected client with its peer list and the range it
    /// allocates network object ids from.
    SelfConnected {
        client_ids: Vec<ClientId>,
        id_range: NetworkIdRange,
    },
    /// Broadcast when a new peer joins.
    ClientConnected { client_id: ClientId },
    /// Broadcast when a peer disconnects.
    ClientDisconnected { client_id: ClientId },
    // SYNCHRONIZATION EVENTS
    /// Periodic host-to-client clock synchronization message.
    SyncTime { current_time: AbsoluteTime },
    /// Replicated component state for one network object.
    SyncComponent {
        time: AbsoluteTime,
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
    /// Enables or disables one component of a replicated object.
    SetComponentEnabled {
        network_object_id: NetworkObjectId,
        from_client_id: ClientId,
        component_uuid: Uuid,
        enabled: bool,
    },
}

impl GameMessage {
    const PROTOCOL_SCHEMA: &str = concat!(
        "ServerEvent{ClientConnected(client_id:ClientId),",
        "ClientDisconnected(client_id:ClientId)};",
        "GameMessage{ServerEvent(ServerEvent),",
        "SelfConnected(client_ids:Vec<ClientId>,id_range:NetworkIdRange),",
        "ClientConnected(client_id:ClientId),",
        "ClientDisconnected(client_id:ClientId),",
        "SyncTime(current_time:AbsoluteTime),",
        "SyncComponent(time:AbsoluteTime,network_object_id:NetworkObjectId,",
        "from_client_id:ClientId,component_uuid:Uuid,data:Vec<u8>),",
        "TransferOwnership(network_object_id:NetworkObjectId,",
        "from_client_id:ClientId,to_client_id:ClientId),",
        "SpawnPrefab(network_object_ids:HashMap<u32,NetworkObjectId>,",
        "from_client_id:ClientId,prefab_id:Uuid),",
        "DestroyGameObject(network_object_id:NetworkObjectId,from_client_id:ClientId),",
        "SetComponentEnabled(network_object_id:NetworkObjectId,from_client_id:ClientId,",
        "component_uuid:Uuid,enabled:bool)}",
    );

    /// Stable FNV-1a hash of the protocol schema string.
    ///
    /// Both client and server use this to reject incompatible builds.
    pub const PROTOCOL_ID: u64 = fnv1a64(Self::PROTOCOL_SCHEMA.as_bytes());

    /// Returns the client the message claims to come from, for the variants
    /// that carry one.
    pub fn claimed_sender(&self) -> Option<ClientId> {
        match self {
            Self::SyncComponent { from_client_id, .. }
            | Self::TransferOwnership { from_client_id, .. }
            | Self::SpawnPrefab { from_client_id, .. }
            | Self::DestroyGameObject { from_client_id, .. }
            | Self::SetComponentEnabled { from_client_id, .. } => Some(*from_client_id),
            Self::ServerEvent(_)
            | Self::SelfConnected { .. }
            | Self::ClientConnected { .. }
            | Self::ClientDisconnected { .. }
            | Self::SyncTime { .. } => None,
        }
    }

    /// Checks a message the server received from client `sender` over the
    /// transport.
    ///
    /// A client may only send messages that carry a sender id, and that id
    /// must be its own transport id. Anything else is spoofed or not meant
    /// for the server.
    pub fn check_client_origin(&self, sender: ClientId) -> Result<(), String> {
        match self.claimed_sender() {
            Some(claimed) if claimed == sender => Ok(()),
            Some(claimed) => Err(format!(
                "claims sender {claimed} but the transport reports {sender}"
            )),
            None => Err(String::from("is a server-only message")),
        }
    }

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
            | Self::DestroyGameObject { .. }
            | Self::SetComponentEnabled { .. } => GameChannel::ReliableOrdered,
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

/// Peer that delivered a queued message, as the transport reported it.
///
/// This is the authority for who sent a message: unlike a sender id inside
/// the message, a client cannot choose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSender {
    /// Produced by this process, such as a server lifecycle event.
    Local,
    /// Received by the local client from the server.
    Server,
    /// Received by the local server from the client with this transport id.
    Client(ClientId),
}

impl MessageSender {
    /// Returns the client a message acts for.
    ///
    /// A message the server received from a client acts for that client's
    /// transport id, whatever id it claims. Messages relayed by the server
    /// or produced locally act for `claimed`, which the server checked on
    /// intake.
    pub fn acting_client(self, claimed: ClientId) -> ClientId {
        match self {
            Self::Client(client_id) => client_id,
            Self::Local | Self::Server => claimed,
        }
    }
}

/// Destination of a message meant for one synchronized component of one
/// network object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageRoute {
    /// Network object the message targets.
    pub network_object_id: NetworkObjectId,
    /// Synchronized component of that object the message targets.
    pub component_uuid: Uuid,
}

/// Messages that may be addressed to one [`MessageRoute`] instead of to
/// every handler.
pub trait RoutedMessage {
    /// Returns the route of a message meant for one synchronized component,
    /// or `None` for a message any handler may consume.
    fn route(&self) -> Option<MessageRoute>;
}

impl RoutedMessage for GameMessage {
    fn route(&self) -> Option<MessageRoute> {
        match self {
            Self::SyncComponent {
                network_object_id,
                component_uuid,
                ..
            } => Some(MessageRoute {
                network_object_id: *network_object_id,
                component_uuid: *component_uuid,
            }),
            _ => None,
        }
    }
}

/// Queued message with the peer that delivered it.
type QueuedMessage<T> = (MessageSender, T);

/// FIFO queues of decoded network messages waiting to be handled by engine
/// systems, each message stamped with the peer that delivered it.
///
/// Messages wait in one bucket per [`RoutedMessage::route`]: the `None`
/// bucket is shared by system-level handlers, and each `Some(route)` bucket
/// holds one synchronized component's messages, so a component reads only
/// its own through [`MessageQueue::receive_messages`].
#[derive(Debug)]
pub struct MessageQueue<T> {
    messages: HashMap<Option<MessageRoute>, VecDeque<QueuedMessage<T>>>,
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
    /// Handles `message`, delivered by `sender`.
    fn handle_message(
        &mut self,
        context: &mut C,
        sender: MessageSender,
        message: &M,
    ) -> MessageHandlerResult;
}

/// Outcome of handling one queued message.
pub enum MessageHandlerResult {
    /// Message was handled and should be removed from the queue.
    Consume,
    /// Message was not handled and should remain queued for another handler
    /// until the end of the tick.
    Ignore,
}

impl<T: RoutedMessage> MessageQueue<T> {
    /// Appends a decoded message delivered by `sender` to the bucket of its
    /// route.
    pub fn queue_message(&mut self, sender: MessageSender, message: T) {
        self.messages
            .entry(message.route())
            .or_default()
            .push_back((sender, message));
    }
}

impl<T> MessageQueue<T> {
    /// Number of queued messages across all buckets.
    pub fn len(&self) -> usize {
        self.messages.values().map(VecDeque::len).sum()
    }

    /// Returns whether no message is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Dispatches the messages queued for `route` to `handler`; `None` is the
    /// bucket of messages that have no route.
    ///
    /// Messages that return [`MessageHandlerResult::Consume`] are removed.
    /// Messages that return [`MessageHandlerResult::Ignore`] stay queued for a
    /// later pass until [`MessageQueue::expire`] drops them.
    pub fn receive_messages<C, H: MessageHandler<C, T>>(
        &mut self,
        route: Option<MessageRoute>,
        context: &mut C,
        handler: &mut H,
    ) {
        let Some(messages) = self.messages.get_mut(&route) else {
            return;
        };
        messages.retain(|(sender, msg)| {
            matches!(
                handler.handle_message(context, *sender, msg),
                MessageHandlerResult::Ignore
            )
        });
        if messages.is_empty() {
            self.messages.remove(&route);
        }
    }

    /// Drops every queued message and returns how many were dropped.
    ///
    /// Call this once every handler has had its pass, so messages no handler
    /// consumes do not pile up.
    pub fn expire(&mut self) -> usize {
        let expired = self.len();
        self.messages.clear();
        expired
    }
}
