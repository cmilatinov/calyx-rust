use crate::context::GameContext;
use crate::test_utils::test_game_context;
use std::time::Duration;

const MAX_CONNECT_ITERS: usize = 100;

pub struct TestHarness {
    pub contexts: Vec<GameContext>,
}

impl TestHarness {
    /// Creates a harness with 1 host + `client_count` clients, all connected via loopback.
    pub fn new(client_count: usize) -> Self {
        let mut host = test_game_context();
        host.resources
            .network_mut()
            .host("127.0.0.1:0".parse().unwrap())
            .expect("failed to host");

        let server_addr = host
            .resources
            .network()
            .server
            .as_ref()
            .unwrap()
            .bound_addr();

        let mut contexts = vec![host];
        for _ in 0..client_count {
            let mut ctx = test_game_context();
            ctx.resources
                .network_mut()
                .client
                .connect(server_addr)
                .expect("failed to connect");
            contexts.push(ctx);
        }

        Self { contexts }
    }

    pub fn host(&self) -> &GameContext {
        &self.contexts[0]
    }

    pub fn host_mut(&mut self) -> &mut GameContext {
        &mut self.contexts[0]
    }

    pub fn client(&self, index: usize) -> &GameContext {
        &self.contexts[index + 1]
    }

    pub fn client_mut(&mut self, index: usize) -> &mut GameContext {
        &mut self.contexts[index + 1]
    }

    /// Tick all contexts once (time + network + scene).
    pub fn pump(&mut self) {
        for ctx in &mut self.contexts {
            ctx.update();
        }
    }

    /// Pump until all clients report connected, returns true on success.
    pub fn wait_connected(&mut self) -> bool {
        for _ in 0..MAX_CONNECT_ITERS {
            self.pump();
            let all_connected = self.contexts[1..]
                .iter()
                .all(|ctx| ctx.resources.network().client.is_connected());
            if all_connected {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    pub fn client_count(&self) -> usize {
        self.contexts.len() - 1
    }
}
