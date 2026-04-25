#[cfg(test)]
mod tests {
    use crate::net::{
        Client, ComponentNetworkObject, GameMessage, MessageHandler, MessageHandlerResult,
        MessageQueue, Network, NetworkSceneSync, Server,
    };
    use crate::test_harness::TestHarness;
    use crate::test_utils::test_scene;
    use std::collections::HashSet;
    use std::time::Duration;

    const TICK: Duration = Duration::from_millis(16);

    struct TestNetwork {
        server: Network,
        client: Network,
    }

    impl TestNetwork {
        fn new() -> Self {
            let mut server = Network::new(60.0);
            server
                .host("127.0.0.1:0".parse().unwrap())
                .expect("failed to host");

            let server_addr = server.server.as_ref().unwrap().bound_addr();

            let mut client = Network::new(60.0);
            client
                .client
                .connect(server_addr)
                .expect("failed to connect");

            Self { server, client }
        }

        /// Pump both sides until the client is connected or max iterations reached.
        fn connect(&mut self) -> bool {
            for _ in 0..100 {
                self.server
                    .server
                    .as_mut()
                    .unwrap()
                    .update(&mut self.server.queue, TICK);
                self.client.client.update(&mut self.client.queue, TICK);
                if self.client.client.is_connected() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            false
        }

        fn pump(&mut self) {
            self.server
                .server
                .as_mut()
                .unwrap()
                .update(&mut self.server.queue, TICK);
            self.client.client.update(&mut self.client.queue, TICK);
        }
    }

    // --- Unit tests ---

    #[test]
    fn network_ids_are_unique_and_monotonic() {
        let id1 = Network::new_id();
        let id2 = Network::new_id();
        let id3 = Network::new_id();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert!(id2 > id1);
        assert!(id3 > id2);
    }

    #[test]
    fn generated_client_ids_are_non_zero_and_unique() {
        let ids: Vec<_> = (0..32).map(|_| Client::generate_client_id()).collect();
        assert!(ids.iter().all(|id| *id != 0), "client ID 0 is reserved");

        let unique_ids: HashSet<_> = ids.iter().copied().collect();
        assert_eq!(unique_ids.len(), ids.len());
    }

    #[test]
    fn network_construction() {
        let network = Network::new(60.0);
        assert_eq!(network.tick_rate(), 60.0);
        assert!((network.tick_period() - 1.0 / 60.0).abs() < 1e-5);
        assert!(!network.is_host());
    }

    #[test]
    fn message_queue_consume() {
        let mut queue = MessageQueue::default();
        queue.queue_message(GameMessage::SyncTime { current_time: 1.0 });
        queue.queue_message(GameMessage::SyncTime { current_time: 2.0 });

        struct ConsumeAll;
        impl MessageHandler<(), GameMessage> for ConsumeAll {
            fn handle_message(&mut self, _: &mut (), _: &GameMessage) -> MessageHandlerResult {
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(&mut (), &mut ConsumeAll);

        // Verify empty
        let mut count = 0usize;
        struct Counter;
        impl MessageHandler<usize, GameMessage> for Counter {
            fn handle_message(&mut self, c: &mut usize, _: &GameMessage) -> MessageHandlerResult {
                *c += 1;
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(&mut count, &mut Counter);
        assert_eq!(count, 0);
    }

    #[test]
    fn message_queue_ignore_retains() {
        let mut queue = MessageQueue::default();
        queue.queue_message(GameMessage::SyncTime { current_time: 1.0 });

        struct IgnoreAll;
        impl MessageHandler<(), GameMessage> for IgnoreAll {
            fn handle_message(&mut self, _: &mut (), _: &GameMessage) -> MessageHandlerResult {
                MessageHandlerResult::Ignore
            }
        }
        queue.receive_messages(&mut (), &mut IgnoreAll);

        let mut count = 0usize;
        struct Counter;
        impl MessageHandler<usize, GameMessage> for Counter {
            fn handle_message(&mut self, c: &mut usize, _: &GameMessage) -> MessageHandlerResult {
                *c += 1;
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(&mut count, &mut Counter);
        assert_eq!(count, 1);
    }

    #[test]
    fn network_object_component() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let client_id = 42u64;
        scene.add_component(
            go,
            ComponentNetworkObject {
                id: 100,
                owner_id: client_id,
            },
        );

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(client_id)
        );
        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.id),
            Some(100)
        );
    }

    #[test]
    fn transfer_ownership_via_handler() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let client_a = 1u64;
        let client_b = 2u64;
        scene.add_component(
            go,
            ComponentNetworkObject {
                id: 42,
                owner_id: client_a,
            },
        );

        let mut client = Client::default();
        let mut server: Option<Server> = None;
        let mut local_id: Option<renet::ClientId> = None;
        let msg = GameMessage::TransferOwnership {
            network_object_id: 42,
            from_client_id: client_a,
            to_client_id: client_b,
        };
        let mut ctx = (&mut client, &mut server, &mut &mut scene, &mut local_id);
        NetworkSceneSync.handle_message(&mut ctx, &msg);

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(client_b)
        );
    }

    // --- Integration tests ---

    #[test]
    fn server_and_client_connect() {
        let mut net = TestNetwork::new();
        assert!(net.connect(), "client failed to connect to server");
        assert!(net.client.client.is_connected());
    }

    #[test]
    fn server_detects_client_connection() {
        let mut net = TestNetwork::new();
        assert!(net.connect());

        // Pump once more so server processes the connection event
        net.pump();

        let server = net.server.server.as_ref().unwrap();
        assert!(server.client_ids().len() >= 1);
    }

    #[test]
    fn client_receives_message_from_server() {
        let mut net = TestNetwork::new();
        assert!(net.connect());
        net.pump();

        // Server broadcasts a message
        let client_id = net
            .server
            .server
            .as_ref()
            .unwrap()
            .client_ids()
            .into_iter()
            .next()
            .expect("no clients connected");

        net.server
            .server
            .as_mut()
            .unwrap()
            .send_message(
                client_id,
                renet::DefaultChannel::ReliableOrdered,
                &GameMessage::SyncTime { current_time: 99.0 },
            )
            .unwrap();

        // Pump to deliver
        for _ in 0..10 {
            net.pump();
            std::thread::sleep(Duration::from_millis(1));
        }

        // Check client received it
        let mut received_time = None;
        struct TimeCapture;
        impl MessageHandler<Option<f32>, GameMessage> for TimeCapture {
            fn handle_message(
                &mut self,
                out: &mut Option<f32>,
                msg: &GameMessage,
            ) -> MessageHandlerResult {
                if let GameMessage::SyncTime { current_time } = msg {
                    *out = Some(*current_time);
                    MessageHandlerResult::Consume
                } else {
                    MessageHandlerResult::Ignore
                }
            }
        }
        net.client
            .queue
            .receive_messages(&mut received_time, &mut TimeCapture);
        assert_eq!(received_time, Some(99.0));
    }

    // --- TestHarness integration tests ---

    #[test]
    fn harness_all_clients_connect() {
        let mut harness = TestHarness::new(3);
        assert!(harness.wait_connected(), "not all clients connected");
        for i in 0..harness.client_count() {
            assert!(harness.client(i).resources.network().client.is_connected());
        }
    }

    #[test]
    fn harness_server_sees_all_clients() {
        let mut harness = TestHarness::new(2);
        assert!(harness.wait_connected());
        harness.pump();
        let server = harness.host().resources.network().server.as_ref().unwrap();
        assert_eq!(server.client_ids().len(), 2);
    }

    #[test]
    fn harness_client_receives_message() {
        let mut harness = TestHarness::new(1);
        assert!(harness.wait_connected());
        harness.pump();

        // Get the client's renet client_id as seen by the server
        let client_ctx = harness.client(0);
        let client_net_id = client_ctx.resources.network().client.client_id().unwrap();

        // Server sends a message directly
        let host_net = harness.host_mut().resources.network_mut();
        host_net
            .server
            .as_mut()
            .unwrap()
            .send_message(
                client_net_id,
                renet::DefaultChannel::ReliableOrdered,
                &GameMessage::SyncTime { current_time: 42.0 },
            )
            .unwrap();

        // Pump transport only (not full update, which would consume SyncTime)
        for _ in 0..10 {
            {
                let net = harness.host_mut().resources.network_mut();
                let tick = Duration::from_millis(16);
                net.server.as_mut().unwrap().update(&mut net.queue, tick);
            }
            {
                let net = harness.client_mut(0).resources.network_mut();
                let tick = Duration::from_millis(16);
                net.client.update(&mut net.queue, tick);
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        let mut received_time = None;
        struct Capture;
        impl MessageHandler<Option<f32>, GameMessage> for Capture {
            fn handle_message(
                &mut self,
                out: &mut Option<f32>,
                msg: &GameMessage,
            ) -> MessageHandlerResult {
                if let GameMessage::SyncTime { current_time } = msg {
                    *out = Some(*current_time);
                    MessageHandlerResult::Consume
                } else {
                    MessageHandlerResult::Ignore
                }
            }
        }
        harness
            .client_mut(0)
            .resources
            .network_mut()
            .queue
            .receive_messages(&mut received_time, &mut Capture);
        assert_eq!(received_time, Some(42.0));
    }
}
