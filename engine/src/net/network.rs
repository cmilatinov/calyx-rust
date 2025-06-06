use crate as engine;
use crate::core::TimeType;
use crate::error::BoxedError;
use crate::net::client::Client;
use crate::net::server::Server;
use crate::scene::Scene;
use engine_derive::Resource;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Resource)]
pub struct Network {
    pub client: Client,
    pub server: Option<Server>,
    tick_period: TimeType,
    accumulated_time: TimeType,
}

impl Default for Network {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

impl Network {
    const DEFAULT_TICK_RATE_HZ: f32 = 120.0;

    pub fn new(tick_rate: f32) -> Self {
        assert!(tick_rate > 0.0);
        Self {
            client: Default::default(),
            server: None,
            tick_period: 1.0 / tick_rate,
            accumulated_time: 0.0,
        }
    }

    pub fn host(&mut self, socket_addr: SocketAddr) -> Result<(), BoxedError> {
        self.server = Some(Server::new(socket_addr)?);
        Ok(())
    }

    pub fn update(&mut self, scene: &mut Scene, duration: Duration) {
        self.accumulated_time += duration.as_secs_f32();
        while self.accumulated_time >= self.tick_period {
            self.accumulated_time -= self.tick_period;
            let duration = Duration::from_secs_f32(self.tick_period);
            if let Some(server) = &mut self.server {
                server.update(scene, duration);
            }
            self.client.update(scene, duration);
        }
    }

    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }

    pub fn tick_period(&self) -> TimeType {
        self.tick_period
    }

    pub fn tick_rate(&self) -> TimeType {
        1.0 / self.tick_period
    }
}
