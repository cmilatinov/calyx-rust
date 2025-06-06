use crate::error::BoxedError;
use crate::net::message::GameMessage;
use crate::scene::Scene;
use renet::{DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

pub struct Client {
    client: RenetClient,
    transport: Option<NetcodeClientTransport>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            client: RenetClient::new(Default::default()),
            transport: None,
        }
    }
}

impl Client {
    pub fn new() -> Self {
        Self {
            client: RenetClient::new(Default::default()),
            transport: None,
        }
    }

    pub fn connect(&mut self, server_addr: SocketAddr) -> Result<(), BoxedError> {
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(Box::new)?;
        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(Box::new)?;
        let client_id = current_time.as_millis() as u64;
        let authentication = ClientAuthentication::Unsecure {
            server_addr,
            client_id,
            user_data: None,
            protocol_id: GameMessage::PROTOCOL_ID,
        };
        self.transport = Some(
            NetcodeClientTransport::new(current_time, authentication, socket).map_err(Box::new)?,
        );
        Ok(())
    }

    pub fn update(&mut self, scene: &mut Scene, duration: Duration) {
        let Self { client, transport } = self;
        client.update(duration);
        if let Some(transport) = transport {
            if let Err(err) = transport.update(duration, client) {
                println!("CLIENT - Error updating transport: {:?}", err);
            }
            if let Err(err) = transport.send_packets(client) {
                println!("CLIENT - Error sending packets: {}", err);
            }
        }

        while let Some(message) = client
            .receive_message(DefaultChannel::ReliableOrdered)
            .and_then(|bytes| bincode::deserialize::<GameMessage>(&bytes).ok())
        {
            println!("CLIENT - Received message: {:?}", message);
        }
    }

    pub fn send_message(&mut self, message: &GameMessage) -> Result<(), BoxedError> {
        self.client.send_message(
            DefaultChannel::ReliableOrdered,
            bincode::serialize(message).map_err(Box::new)?,
        );
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    pub fn is_connecting(&self) -> bool {
        self.client.is_connecting()
    }

    pub fn is_disconnected(&self) -> bool {
        self.client.is_disconnected()
    }

    pub fn rtt(&self) -> Duration {
        Duration::from_secs_f64(self.client.rtt())
    }
}
