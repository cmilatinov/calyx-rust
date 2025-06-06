use crate::error::BoxedError;
use crate::net::message::GameMessage;
use crate::scene::Scene;
use renet::{ClientId, ConnectionConfig, DefaultChannel, RenetServer, ServerEvent};
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

    pub fn update(&mut self, scene: &mut Scene, duration: Duration) {
        let Self { server, transport } = self;
        server.update(duration);
        if let Err(err) = transport.update(duration, server) {
            println!("SERVER - Error updating transport: {:?}", err);
        }

        while let Some(event) = server.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    println!("SERVER - Client connected: {}", client_id);
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    println!("SERVER - Client disconnected: {}", client_id);
                    println!("SERVER - Reason: {}", reason);
                }
            }
        }

        for client_id in server.clients_id() {
            while let Some(message) = server
                .receive_message(client_id, DefaultChannel::ReliableOrdered)
                .and_then(|bytes| bincode::deserialize::<GameMessage>(&bytes).ok())
            {
                println!("SERVER - Received message: {:?}", message);
            }
        }

        transport.send_packets(server);
    }

    pub fn send_message<I: Into<u8>>(
        &mut self,
        client_id: ClientId,
        channel_id: I,
        message: &GameMessage,
    ) -> Result<(), BoxedError> {
        let bytes = bincode::serialize(message).map_err(Box::new)?;
        self.server.send_message(client_id, channel_id, bytes);
        Ok(())
    }

    pub fn receive_message<I: Into<u8>>(
        &mut self,
        client_id: ClientId,
        channel_id: I,
    ) -> Option<GameMessage> {
        self.server
            .receive_message(client_id, channel_id)
            .and_then(|msg| bincode::deserialize(&msg).ok())
    }
}
