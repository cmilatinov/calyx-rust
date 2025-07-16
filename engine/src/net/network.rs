use crate as engine;
use crate::core::{Time, TimeType};
use crate::error::BoxedError;
use crate::net::client::Client;
use crate::net::server::Server;
use crate::net::{GameMessage, MessageHandler, MessageHandlerResult, MessageQueue};
use engine_derive::Resource;
use renet::DefaultChannel;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Resource)]
pub struct Network {
    pub client: Client,
    pub server: Option<Server>,
    pub queue: MessageQueue<GameMessage>,
    tick_rate: TimeType,
    tick_period: TimeType,
    accumulated_time: TimeType,
}

impl Default for Network {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

impl Network {
    const DEFAULT_TICK_RATE_HZ: f32 = 20.0;

    pub fn new(tick_rate: f32) -> Self {
        assert!(tick_rate > 0.0);
        Self {
            client: Default::default(),
            server: None,
            queue: Default::default(),
            tick_rate,
            tick_period: 1.0 / tick_rate,
            accumulated_time: 0.0,
        }
    }

    pub fn host(&mut self, socket_addr: SocketAddr) -> Result<(), BoxedError> {
        self.server = Some(Server::new(socket_addr)?);
        Ok(())
    }

    pub fn update(&mut self, time: &mut Time) {
        self.accumulated_time += time.static_duration().as_secs_f32();
        while self.accumulated_time >= self.tick_period {
            self.accumulated_time -= self.tick_period;
            let duration = Duration::from_secs_f32(self.tick_period);
            if let Some(server) = &mut self.server {
                server.update(&mut self.queue, duration);
                if time.timer("NETWORK_TICK_SYNC") >= 1.0 {
                    let _ = server.broadcast_message_except(
                        0,
                        DefaultChannel::ReliableOrdered,
                        &GameMessage::SyncTime {
                            current_time: time.time,
                        },
                    );
                    time.reset_timer("NETWORK_TICK_SYNC");
                }
            }
            self.client.update(&mut self.queue, duration);
        }
        self.queue
            .receive_messages(&mut (&mut time.time, self.tick_rate), &mut NetworkTimeSync);
    }

    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }

    pub fn tick_period(&self) -> TimeType {
        self.tick_period
    }

    pub fn tick_rate(&self) -> TimeType {
        self.tick_rate
    }
}

struct NetworkTimeSync;
impl MessageHandler<(&mut TimeType, TimeType), GameMessage> for NetworkTimeSync {
    fn handle_message(
        &mut self,
        (time_counter, tick_rate): &mut (&mut TimeType, TimeType),
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SyncTime { current_time } => {
                **time_counter = *current_time + (1.0 / *tick_rate);
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}
