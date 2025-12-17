use crate::error::BoxedError;
use crate::net::message::GameMessage;
use crate::net::MessageQueue;
use log::{error, info, trace};
use renet::{ClientId, DefaultChannel, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

pub struct Client {
    client: RenetClient,
    transport: Option<NetcodeClientTransport>,
    pub(crate) client_ids: Vec<ClientId>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            client: RenetClient::new(Default::default()),
            transport: None,
            client_ids: Default::default(),
        }
    }
}

impl Client {
    pub fn new() -> Self {
        Self {
            client: RenetClient::new(Default::default()),
            transport: None,
            client_ids: Default::default(),
        }
    }

    pub fn connect(&mut self, server_addr: SocketAddr) -> Result<(), BoxedError> {
        info!("Connecting to server at {}", server_addr);
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| {
            error!("Failed to bind socket: {}", e);
            Box::new(e) as Box<dyn std::error::Error + Send + Sync>
        })?;

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| {
                error!("System time error: {}", e);
                Box::new(e) as Box<dyn std::error::Error + Send + Sync>
            })?;

        let client_id = current_time.as_millis() as u64;
        trace!("Generated client ID: {}", client_id);

        let authentication = ClientAuthentication::Unsecure {
            server_addr,
            client_id,
            user_data: None,
            protocol_id: GameMessage::PROTOCOL_ID,
        };

        self.transport = Some(
            NetcodeClientTransport::new(current_time, authentication, socket).map_err(|e| {
                error!("Failed to create transport: {}", e);
                Box::new(e) as Box<dyn std::error::Error + Send + Sync>
            })?,
        );

        info!("Successfully initialized client transport");
        Ok(())
    }

    pub fn update(&mut self, queue: &mut MessageQueue<GameMessage>, duration: Duration) {
        let Self {
            client, transport, ..
        } = self;

        client.update(duration);

        if let Some(transport) = transport {
            if let Err(err) = transport.update(duration, client) {
                error!("Error updating transport: {:?}", err);
            }

            if let Err(err) = transport.send_packets(client) {
                error!("Error sending packets: {}", err);
            }
        }

        while let Some((message, _)) = client
            .receive_message(DefaultChannel::ReliableOrdered)
            .and_then(|bytes| {
                bincode::serde::decode_from_slice::<GameMessage, _>(
                    &bytes,
                    bincode::config::standard(),
                )
                .map_err(|e| {
                    error!("Failed to decode message from server: {}", e);
                    e
                })
                .ok()
            })
        {
            queue.queue_message(message);
        }
    }

    pub fn send_message(&mut self, message: &GameMessage) -> Result<(), BoxedError> {
        trace!("Sending message to server: {:?}", message);
        let bytes =
            bincode::serde::encode_to_vec(message, bincode::config::standard()).map_err(|e| {
                error!("Failed to serialize message: {}", e);
                Box::new(e) as Box<dyn std::error::Error + Send + Sync>
            })?;

        self.client
            .send_message(DefaultChannel::ReliableOrdered, bytes);
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        let connected = self.client.is_connected();
        if connected {
            trace!("Client is connected");
        }
        connected
    }

    pub fn is_connecting(&self) -> bool {
        let connecting = self.client.is_connecting();
        if connecting {
            trace!("Client is connecting...");
        }
        connecting
    }

    pub fn is_disconnected(&self) -> bool {
        let disconnected = self.client.is_disconnected();
        if disconnected {
            trace!("Client is disconnected");
        }
        disconnected
    }

    pub fn rtt(&self) -> Duration {
        Duration::from_secs_f64(self.client.rtt())
    }

    pub fn client_id(&self) -> Option<ClientId> {
        self.transport.as_ref().map(|t| t.client_id())
    }

    pub fn client_ids(&self) -> Vec<ClientId> {
        self.client_ids.clone()
    }
}
