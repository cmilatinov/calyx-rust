use crate::error::BoxedError;
use crate::net::message::{GameChannel, GameMessage};
use crate::net::{MessageQueue, MessageSender, NetworkObjectId, ServerEvent};
use renet::{Bytes, ClientId, ConnectionConfig, RenetServer};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

/// Local authoritative server plus its netcode transport.
pub struct Server {
    server: RenetServer,
    transport: NetcodeServerTransport,
    addr: SocketAddr,
    /// Component syncs received from clients, waiting for
    /// [`Server::relay_component_syncs`] to check their sender's ownership.
    pending_syncs: Vec<PendingSync>,
}

/// Component sync a client sent, kept encoded so the relay forwards the
/// client's bytes unchanged.
struct PendingSync {
    sender: ClientId,
    network_object_id: NetworkObjectId,
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
    pub fn update(&mut self, queue: &mut MessageQueue<GameMessage>, duration: Duration) {
        let Self {
            server,
            transport,
            pending_syncs,
            ..
        } = self;

        server.update(duration);
        if let Err(err) = transport.update(duration, server) {
            log::error!("Error updating server transport: {err:?}");
        }

        while let Some(event) = server.get_event() {
            match &event {
                renet::ServerEvent::ClientConnected { client_id } => {
                    log::info!("Client connected: {client_id}");
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
                        network_object_id, ..
                    } = &message
                    {
                        pending_syncs.push(PendingSync {
                            sender: client_id,
                            network_object_id: *network_object_id,
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

    /// Relays the component syncs received from clients since the last call
    /// to every client but their sender.
    ///
    /// `owner_of` returns the owner of a network object the host holds. A
    /// sync for an object its sender does not own is dropped. The host may not
    /// hold every object, so a sync for an object it does not know is relayed
    /// and each receiving peer checks ownership itself.
    pub fn relay_component_syncs(
        &mut self,
        mut owner_of: impl FnMut(NetworkObjectId) -> Option<ClientId>,
    ) {
        for sync in std::mem::take(&mut self.pending_syncs) {
            match owner_of(sync.network_object_id) {
                Some(owner_id) if owner_id != sync.sender => log::warn!(
                    "Rejected sync of network object {} owned by {owner_id} from {}",
                    sync.network_object_id,
                    sync.sender
                ),
                _ => self
                    .server
                    .broadcast_message_except(sync.sender, sync.channel, sync.bytes),
            }
        }
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
