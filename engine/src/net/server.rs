use crate::error::BoxedError;
use crate::net::message::GameMessage;
use crate::net::{MessageQueue, ServerEvent};
use renet::{ClientId, ConnectionConfig, DefaultChannel, RenetServer};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

pub struct Server {
    server: RenetServer,
    transport: NetcodeServerTransport,
}

impl Server {
    // TODO(Cristian): Remove this, socket address should come from user input
    pub fn addr() -> SocketAddr {
        "127.0.0.1:54321".parse().unwrap()
    }

    pub fn new(socket_addr: SocketAddr) -> Result<Self, BoxedError> {
        let config = ServerConfig {
            current_time: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap(),
            max_clients: 32,
            protocol_id: GameMessage::PROTOCOL_ID,
            public_addresses: vec!["127.0.0.1:0".parse().unwrap()],
            authentication: ServerAuthentication::Unsecure,
        };
        let socket = UdpSocket::bind(socket_addr).map_err(Box::new)?;
        let server = RenetServer::new(ConnectionConfig::default());
        let transport = NetcodeServerTransport::new(config, socket).map_err(Box::new)?;
        Ok(Self { server, transport })
    }

    pub fn update(&mut self, queue: &mut MessageQueue<GameMessage>, duration: Duration) {
        let Self { server, transport } = self;
        server.update(duration);
        if let Err(err) = transport.update(duration, server) {
            println!("SERVER - Error updating transport: {:?}", err);
        }

        while let Some(event) = server.get_event() {
            match &event {
                renet::ServerEvent::ClientConnected { client_id } => {
                    println!("SERVER - Client connected: {}", client_id);
                }
                renet::ServerEvent::ClientDisconnected { client_id, reason } => {
                    println!("SERVER - Client disconnected: {}", client_id);
                    println!("SERVER - Reason: {}", reason);
                }
            }
            queue.queue_message(GameMessage::ServerEvent(match event {
                renet::ServerEvent::ClientConnected { client_id } => {
                    ServerEvent::ClientConnected { client_id }
                }
                renet::ServerEvent::ClientDisconnected { client_id, .. } => {
                    ServerEvent::ClientDisconnected { client_id }
                }
            }));
        }

        for client_id in server.clients_id() {
            while let Some(message) = server
                .receive_message(client_id, DefaultChannel::ReliableOrdered)
                .and_then(|bytes| bincode::deserialize::<GameMessage>(&bytes).ok())
            {
                queue.queue_message(message);
            }
        }

        transport.send_packets(server);
    }

    fn serialize_message(message: &GameMessage) -> Result<Vec<u8>, BoxedError> {
        Ok(bincode::serialize(message).map_err(Box::new)?)
    }

    pub fn broadcast_message<I: Into<u8>>(
        &mut self,
        channel_id: I,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server.broadcast_message(channel_id, bytes);
        Ok(())
    }

    pub fn broadcast_message_except<I: Into<u8>>(
        &mut self,
        except_id: ClientId,
        channel_id: I,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server
            .broadcast_message_except(except_id, channel_id, bytes);
        Ok(())
    }

    pub fn send_message<I: Into<u8>>(
        &mut self,
        client_id: ClientId,
        channel_id: I,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = Self::serialize_message(message)?;
        self.server.send_message(client_id, channel_id, bytes);
        Ok(())
    }

    pub fn client_ids(&self) -> Vec<ClientId> {
        self.server.clients_id()
    }

    pub fn client_ids_iter<'a>(&'a self) -> impl Iterator<Item = ClientId> + 'a {
        self.server.clients_id_iter()
    }
}
