use crate::error::BoxedError;
use crate::net::message::{GameChannel, GameMessage};
use crate::net::{
    MessageQueue, MessageSender, NetworkIdRange, NetworkObjectId, ReplicationRecord, ServerEvent,
    SnapshotPlan, SNAPSHOT_CHUNK_BYTES, SNAPSHOT_LIVE_HEADROOM_BYTES,
};
use renet::{Bytes, ClientId, ConnectionConfig, RenetServer};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// Local authoritative server plus its netcode transport.
pub struct Server {
    server: RenetServer,
    transport: NetcodeServerTransport,
    addr: SocketAddr,
    /// Component syncs received from clients, waiting for
    /// [`Server::relay_component_syncs`] to check their sender's ownership.
    pending_syncs: Vec<PendingSync>,
    /// Network object id range reserved for each peer that joined this
    /// session, the host included. A range stays reserved after its peer
    /// leaves, since its objects may outlive the connection.
    id_ranges: HashMap<ClientId, NetworkIdRange>,
    /// Next range [`Server::reserve_id_range`] hands out; 0 is never used.
    next_id_range: NetworkIdRange,
    /// Replicated state changed at runtime, sent to each client that joins.
    record: ReplicationRecord,
}

/// Component sync a client sent, kept encoded so the relay forwards the
/// client's bytes unchanged.
struct PendingSync {
    sender: ClientId,
    network_object_id: NetworkObjectId,
    component_uuid: Uuid,
    data: Vec<u8>,
    channel: GameChannel,
    bytes: Bytes,
}

impl Server {
    /// Binds a server socket and starts listening on `socket_addr`.
    pub fn new(socket_addr: SocketAddr) -> Result<Self, BoxedError> {
        let socket = UdpSocket::bind(socket_addr).map_err(Box::new)?;
        let bound_addr = socket.local_addr().map_err(Box::new)?;

        #[cfg(debug_assertions)]
        log::warn!("Using unsecure server authentication (debug build)");

        let config = ServerConfig {
            current_time: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap(),
            max_clients: 32,
            protocol_id: GameMessage::PROTOCOL_ID,
            public_addresses: vec![bound_addr],
            authentication: Self::authentication(),
        };
        let server = RenetServer::new(ConnectionConfig::default());
        let transport = NetcodeServerTransport::new(config, socket).map_err(Box::new)?;
        Ok(Self {
            server,
            transport,
            addr: bound_addr,
            pending_syncs: Vec::new(),
            id_ranges: HashMap::new(),
            next_id_range: 1,
            record: ReplicationRecord::default(),
        })
    }

    /// In debug builds, use unsecure authentication for easy local testing.
    /// Release builds log a warning — replace with secure auth before shipping.
    fn authentication() -> ServerAuthentication {
        #[cfg(not(debug_assertions))]
        log::warn!(
            "ServerAuthentication::Unsecure used in release build — \
             replace with secure authentication before shipping"
        );
        ServerAuthentication::Unsecure
    }

    /// Advances the server, emits connection events, and queues any received
    /// client messages.
    ///
    /// A client that just connected gets its `SelfConnected` and its join
    /// snapshot here, as soon as the transport reports it and before this
    /// call queues, relays or sends anything else. Every live message reaches
    /// it later on the same ordered channel, so it never sees one before the
    /// snapshot, and the snapshot holds every change relayed before it.
    pub fn update(&mut self, queue: &mut MessageQueue<GameMessage>, duration: Duration) {
        self.server.update(duration);
        if let Err(err) = self.transport.update(duration, &mut self.server) {
            log::error!("Error updating server transport: {err:?}");
        }

        while let Some(event) = self.server.get_event() {
            match &event {
                renet::ServerEvent::ClientConnected { client_id } => {
                    log::info!("Client connected: {client_id}");
                    self.welcome_client(*client_id);
                }
                renet::ServerEvent::ClientDisconnected { client_id, reason } => {
                    log::info!("Client disconnected: {client_id}, reason: {reason}");
                }
            }
            queue.queue_message(
                MessageSender::Local,
                GameMessage::ServerEvent(match event {
                    renet::ServerEvent::ClientConnected { client_id } => {
                        ServerEvent::ClientConnected { client_id }
                    }
                    renet::ServerEvent::ClientDisconnected { client_id, .. } => {
                        ServerEvent::ClientDisconnected { client_id }
                    }
                }),
            );
        }

        let Self {
            server,
            transport,
            pending_syncs,
            ..
        } = self;
        for client_id in server.clients_id() {
            for channel in GameChannel::ALL {
                while let Some(bytes) = server.receive_message(client_id, channel) {
                    let message = match bincode::serde::decode_from_slice::<GameMessage, _>(
                        &bytes,
                        bincode::config::standard(),
                    ) {
                        Ok((message, _)) => message,
                        Err(e) => {
                            log::error!("Failed to decode message from client {client_id}: {e}");
                            continue;
                        }
                    };
                    if let Err(reason) = message.check_client_origin(client_id) {
                        log::warn!("Rejected message from client {client_id}: it {reason}");
                        continue;
                    }
                    if let GameMessage::SyncComponent {
                        network_object_id,
                        component_uuid,
                        data,
                        ..
                    } = &message
                    {
                        pending_syncs.push(PendingSync {
                            sender: client_id,
                            network_object_id: *network_object_id,
                            component_uuid: *component_uuid,
                            data: data.clone(),
                            channel: message.channel(),
                            bytes,
                        });
                    }
                    queue.queue_message(MessageSender::Client(client_id), message);
                }
            }
        }

        transport.send_packets(server);
    }

    /// Sends a client that just connected its `SelfConnected`, then its join
    /// snapshot, and tells every other client it joined.
    fn welcome_client(&mut self, client_id: ClientId) {
        let Some(id_range) = self.reserve_id_range(client_id) else {
            log::error!("No network object id range left for client {client_id}");
            return;
        };
        let self_connected = GameMessage::SelfConnected {
            client_ids: self.client_ids(),
            id_range,
        };
        if let Err(e) = self.send_message(client_id, &self_connected) {
            log::warn!("Failed to send SelfConnected to {client_id}: {e}");
        }
        self.send_join_snapshot(client_id);
        if let Err(e) =
            self.broadcast_message_except(client_id, &GameMessage::ClientConnected { client_id })
        {
            log::warn!("Failed to broadcast ClientConnected: {e}");
        }
    }

    /// Sends `client_id` the snapshot of the replicated state changed at
    /// runtime, on the reliable ordered channel.
    ///
    /// The snapshot is split into messages of up to [`SNAPSHOT_CHUNK_BYTES`].
    /// All of it must fit in the channel's free memory, less
    /// [`SNAPSHOT_LIVE_HEADROOM_BYTES`] for live traffic, or renet would
    /// disconnect the client. A larger snapshot is cut to its longest prefix
    /// that fits, with a warning that names its size.
    fn send_join_snapshot(&mut self, client_id: ClientId) {
        let messages = self.record.messages(SNAPSHOT_CHUNK_BYTES);
        if messages.is_empty() {
            return;
        }
        let channel = GameChannel::ReliableOrdered;
        let budget = self
            .server
            .channel_available_memory(client_id, channel)
            .saturating_sub(SNAPSHOT_LIVE_HEADROOM_BYTES);
        let plan = SnapshotPlan::new(&messages, budget);
        if plan.is_truncated() {
            log::warn!(
                "Join snapshot for client {client_id} is {} bytes, over its {budget}-byte \
                 budget on the reliable channel; sending the first {} bytes, so the client \
                 misses the rest of the replicated state",
                plan.total_bytes,
                plan.sent_bytes
            );
        } else {
            log::debug!(
                "Sending client {client_id} a join snapshot of {} bytes in {} messages",
                plan.total_bytes,
                plan.messages.len()
            );
        }
        for bytes in plan.messages {
            self.server.send_message(client_id, channel, bytes);
        }
    }

    /// Relays the component syncs received from clients since the last call
    /// to every client but their sender, and keeps each relayed payload as
    /// its component's latest for the join snapshot.
    ///
    /// `owner_of` returns the owner of a network object the host holds. A
    /// sync for an object its sender does not own is dropped. The host may not
    /// hold every object; a sync for an object it does not hold is checked
    /// against the owner the record keeps, and one for an object neither
    /// knows is relayed without being kept, so each receiving peer checks
    /// ownership itself.
    pub fn relay_component_syncs(
        &mut self,
        mut owner_of: impl FnMut(NetworkObjectId) -> Option<ClientId>,
    ) {
        for sync in std::mem::take(&mut self.pending_syncs) {
            let owner = owner_of(sync.network_object_id)
                .or_else(|| self.record.owner(sync.network_object_id));
            match owner {
                Some(owner_id) if owner_id != sync.sender => {
                    log::warn!(
                        "Rejected sync of network object {} owned by {owner_id} from {}",
                        sync.network_object_id,
                        sync.sender
                    );
                    continue;
                }
                Some(_) => self.record.component_sync(
                    sync.network_object_id,
                    sync.component_uuid,
                    &sync.data,
                ),
                None => {}
            }
            self.server
                .broadcast_message_except(sync.sender, sync.channel, sync.bytes);
        }
    }

    /// The replicated state the server accepted at runtime, which it sends
    /// each client that joins.
    pub fn record(&self) -> &ReplicationRecord {
        &self.record
    }

    /// Mutable access to [`Server::record`], for the handlers that accept
    /// replicated changes and the host's own changes.
    pub(crate) fn record_mut(&mut self) -> &mut ReplicationRecord {
        &mut self.record
    }

    /// Returns the network object id range reserved for `client_id`,
    /// reserving the next unused one on first call. Returns `None` once every
    /// range is used up.
    pub fn reserve_id_range(&mut self, client_id: ClientId) -> Option<NetworkIdRange> {
        if let Some(&range) = self.id_ranges.get(&client_id) {
            return Some(range);
        }
        let range = self.next_id_range;
        self.next_id_range = range.checked_add(1)?;
        self.id_ranges.insert(client_id, range);
        Some(range)
    }

    /// Returns the network object id range reserved for `client_id`, if any.
    pub fn id_range(&self, client_id: ClientId) -> Option<NetworkIdRange> {
        self.id_ranges.get(&client_id).copied()
    }

    /// Drops the component syncs no [`Server::relay_component_syncs`] call
    /// relayed.
    pub(crate) fn discard_component_syncs(&mut self) {
        self.pending_syncs.clear();
    }

    /// Serializes one protocol message into bytes suitable for Renet.
    fn serialize_message(message: &GameMessage) -> Result<Vec<u8>, BoxedError> {
        Ok(
            bincode::serde::encode_to_vec(message, bincode::config::standard())
                .map_err(Box::new)?,
        )
    }

    /// Broadcasts `message` to every connected client.
    pub fn broadcast_message(&mut self, message: &GameMessage) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server.broadcast_message(message.channel(), bytes);
        Ok(())
    }

    /// Broadcasts `message` to every client except `except_id`.
    pub fn broadcast_message_except(
        &mut self,
        except_id: ClientId,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server
            .broadcast_message_except(except_id, message.channel(), bytes);
        Ok(())
    }

    /// Sends `message` to one client.
    pub fn send_message(
        &mut self,
        client_id: ClientId,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server
            .send_message(client_id, message.channel(), bytes);
        Ok(())
    }

    /// Returns a snapshot of all connected client IDs.
    pub fn client_ids(&self) -> Vec<ClientId> {
        self.server.clients_id()
    }

    /// Iterates connected client IDs without allocating a vector.
    pub fn client_ids_iter<'a>(&'a self) -> impl Iterator<Item = ClientId> + 'a {
        self.server.clients_id_iter()
    }

    /// Returns the public addresses reported by the netcode transport.
    pub fn addresses(&self) -> Vec<SocketAddr> {
        self.transport.addresses()
    }

    /// Returns the bound local socket address.
    pub fn bound_addr(&self) -> SocketAddr {
        self.addr
    }
}
