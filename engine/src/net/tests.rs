#[cfg(test)]
mod tests {
    use crate::context::GameContext;
    use crate::core::Ref;
    use crate::net::{
        handle_destroy, Client, ComponentNetworkObject, GameChannel, GameMessage, MessageHandler,
        MessageHandlerResult, MessageQueue, MessageRoute, MessageSender, Network, NetworkSceneSync,
        Server,
    };
    use crate::scene::{GameObject, Prefab, Scene};
    use crate::test_harness::TestHarness;
    use crate::test_utils::{test_game_context, test_scene};
    use std::collections::HashSet;
    use std::time::Duration;

    const TICK: Duration = Duration::from_millis(16);

    /// One server and any number of clients running in this process over loopback.
    struct TestNetwork {
        server: Network,
        clients: Vec<Network>,
    }

    impl TestNetwork {
        fn new() -> Self {
            Self::with_clients(1)
        }

        fn with_clients(client_count: usize) -> Self {
            let mut server = Network::new(60.0);
            server
                .host("127.0.0.1:0".parse().unwrap())
                .expect("failed to host");

            let server_addr = server.server.as_ref().unwrap().bound_addr();

            let clients = (0..client_count)
                .map(|_| {
                    let mut client = Network::new(60.0);
                    client
                        .client
                        .connect(server_addr)
                        .expect("failed to connect");
                    client
                })
                .collect();

            Self { server, clients }
        }

        /// Pump every peer until all clients are connected or max iterations reached.
        fn connect(&mut self) -> bool {
            for _ in 0..100 {
                self.pump();
                if self.clients.iter().all(|c| c.client.is_connected()) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            false
        }

        /// Run one transport update on the server and on every client.
        fn pump(&mut self) {
            self.server
                .server
                .as_mut()
                .unwrap()
                .update(&mut self.server.queue, TICK);
            for client in &mut self.clients {
                client.client.update(&mut client.queue, TICK);
            }
        }

        /// Pump all peers until a number of consecutive rounds pass with no
        /// change in any queue length, meaning in-flight messages have landed.
        fn pump_until_drained(&mut self) {
            const QUIET_ROUNDS: usize = 5;
            let mut quiet = 0;
            let mut last = self.queue_lengths();
            for _ in 0..200 {
                self.pump();
                std::thread::sleep(Duration::from_millis(1));
                let current = self.queue_lengths();
                if current == last {
                    quiet += 1;
                    if quiet >= QUIET_ROUNDS {
                        return;
                    }
                } else {
                    quiet = 0;
                    last = current;
                }
            }
        }

        fn queue_lengths(&self) -> Vec<usize> {
            std::iter::once(&self.server)
                .chain(self.clients.iter())
                .map(|n| n.queue.len())
                .collect()
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
        queue.queue_message(
            MessageSender::Server,
            GameMessage::SyncTime { current_time: 1.0 },
        );
        queue.queue_message(
            MessageSender::Server,
            GameMessage::SyncTime { current_time: 2.0 },
        );

        struct ConsumeAll;
        impl MessageHandler<(), GameMessage> for ConsumeAll {
            fn handle_message(
                &mut self,
                _: &mut (),
                _: MessageSender,
                _: &GameMessage,
            ) -> MessageHandlerResult {
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(None, &mut (), &mut ConsumeAll);

        // Verify empty
        let mut count = 0usize;
        struct Counter;
        impl MessageHandler<usize, GameMessage> for Counter {
            fn handle_message(
                &mut self,
                c: &mut usize,
                _: MessageSender,
                _: &GameMessage,
            ) -> MessageHandlerResult {
                *c += 1;
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(None, &mut count, &mut Counter);
        assert_eq!(count, 0);
    }

    #[test]
    fn message_queue_ignore_retains() {
        let mut queue = MessageQueue::default();
        queue.queue_message(
            MessageSender::Server,
            GameMessage::SyncTime { current_time: 1.0 },
        );

        struct IgnoreAll;
        impl MessageHandler<(), GameMessage> for IgnoreAll {
            fn handle_message(
                &mut self,
                _: &mut (),
                _: MessageSender,
                _: &GameMessage,
            ) -> MessageHandlerResult {
                MessageHandlerResult::Ignore
            }
        }
        queue.receive_messages(None, &mut (), &mut IgnoreAll);

        let mut count = 0usize;
        struct Counter;
        impl MessageHandler<usize, GameMessage> for Counter {
            fn handle_message(
                &mut self,
                c: &mut usize,
                _: MessageSender,
                _: &GameMessage,
            ) -> MessageHandlerResult {
                *c += 1;
                MessageHandlerResult::Consume
            }
        }
        queue.receive_messages(None, &mut count, &mut Counter);
        assert_eq!(count, 1);
    }

    fn sync_component(network_object_id: u64, component_uuid: uuid::Uuid) -> GameMessage {
        GameMessage::SyncComponent {
            time: 0.0,
            network_object_id,
            from_client_id: 1,
            component_uuid,
            data: Vec::new(),
        }
    }

    /// Counts and consumes every message it is offered.
    struct ConsumeCounter;
    impl MessageHandler<usize, GameMessage> for ConsumeCounter {
        fn handle_message(
            &mut self,
            count: &mut usize,
            _: MessageSender,
            _: &GameMessage,
        ) -> MessageHandlerResult {
            *count += 1;
            MessageHandlerResult::Consume
        }
    }

    #[test]
    fn component_sync_messages_are_routed_by_object_and_component() {
        let component_a = uuid::Uuid::from_u128(1);
        let component_b = uuid::Uuid::from_u128(2);
        let mut queue = MessageQueue::default();
        queue.queue_message(MessageSender::Server, sync_component(7, component_a));
        queue.queue_message(MessageSender::Server, sync_component(7, component_a));
        queue.queue_message(MessageSender::Server, sync_component(7, component_b));
        queue.queue_message(MessageSender::Server, sync_component(8, component_a));
        queue.queue_message(
            MessageSender::Server,
            GameMessage::SyncTime { current_time: 1.0 },
        );

        let mut unrouted = 0;
        queue.receive_messages(None, &mut unrouted, &mut ConsumeCounter);
        assert_eq!(unrouted, 1, "only SyncTime is in the None bucket");

        let mut routed = 0;
        queue.receive_messages(
            Some(MessageRoute {
                network_object_id: 7,
                component_uuid: component_a,
            }),
            &mut routed,
            &mut ConsumeCounter,
        );
        assert_eq!(routed, 2, "a route sees only its own messages");
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn unconsumed_messages_expire_at_end_of_tick() {
        let mut game = test_game_context();
        let queue = &mut game.resources.network_mut().queue;
        queue.queue_message(
            MessageSender::Server,
            sync_component(999, uuid::Uuid::from_u128(1)),
        );
        queue.queue_message(
            MessageSender::Server,
            sync_component(999, uuid::Uuid::from_u128(2)),
        );

        // Every handler gets its pass, and none consumes a message for an
        // object that does not exist.
        let GameContext {
            assets,
            scenes,
            resources,
        } = &mut game;
        resources
            .network_mut()
            .update_scene(scenes.current_scene_mut(), &assets.lock_read());
        assert_eq!(game.resources.network().queue.len(), 2);

        // The next tick starts with the queue cleared.
        game.update();
        assert_eq!(game.resources.network().queue.len(), 0);
    }

    #[test]
    fn protocol_id_is_schema_derived() {
        assert_ne!(GameMessage::PROTOCOL_ID, 42069);
        assert_eq!(GameMessage::PROTOCOL_ID, 0xb5f6_8f80_f311_3c28);
    }

    #[test]
    fn messages_choose_delivery_channels() {
        assert_eq!(
            GameMessage::SyncTime { current_time: 1.0 }.channel(),
            GameChannel::Unreliable
        );

        assert_eq!(
            GameMessage::SyncComponent {
                time: 1.0,
                network_object_id: 7,
                from_client_id: 1,
                component_uuid: uuid::Uuid::nil(),
                data: Vec::new(),
            }
            .channel(),
            GameChannel::Unreliable
        );

        assert_eq!(
            GameMessage::SelfConnected {
                client_ids: Vec::new()
            }
            .channel(),
            GameChannel::ReliableUnordered
        );

        assert_eq!(
            GameMessage::TransferOwnership {
                network_object_id: 7,
                from_client_id: 1,
                to_client_id: 2,
            }
            .channel(),
            GameChannel::ReliableOrdered
        );
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
        NetworkSceneSync.handle_message(&mut ctx, MessageSender::Server, &msg);

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(client_b)
        );
    }

    #[test]
    fn client_origin_requires_own_sender_id() {
        let transfer = |from_client_id| GameMessage::TransferOwnership {
            network_object_id: 7,
            from_client_id,
            to_client_id: 3,
        };
        assert!(transfer(1).check_client_origin(1).is_ok());
        assert!(transfer(2).check_client_origin(1).is_err());
        assert!(GameMessage::SyncTime { current_time: 1.0 }
            .check_client_origin(1)
            .is_err());
    }

    #[test]
    fn transfer_ownership_checks_stamped_sender() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        let (attacker, victim) = (1u64, 2u64);
        scene.add_component(
            go,
            ComponentNetworkObject {
                id: 42,
                owner_id: victim,
            },
        );

        let mut client = Client::default();
        let mut server: Option<Server> = None;
        let mut local_id: Option<renet::ClientId> = None;
        let msg = GameMessage::TransferOwnership {
            network_object_id: 42,
            from_client_id: victim,
            to_client_id: attacker,
        };
        let mut ctx = (&mut client, &mut server, &mut &mut scene, &mut local_id);
        NetworkSceneSync.handle_message(&mut ctx, MessageSender::Client(attacker), &msg);

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(victim)
        );
    }

    #[test]
    fn destroy_from_owner_removes_object() {
        let (mut scene, go) = scene_with_object(42, 1);
        let mut server: Option<Server> = None;
        let msg = GameMessage::DestroyGameObject {
            network_object_id: 42,
            from_client_id: 1,
        };
        handle_destroy(&mut server, &mut scene, MessageSender::Server, 42, 1, &msg);
        scene.flush_deletes();

        assert!(scene.entry(go).is_none(), "owner's destroy was not applied");
    }

    #[test]
    fn destroy_from_non_owner_is_rejected() {
        let (owner, attacker) = (1u64, 2u64);
        let (mut scene, go) = scene_with_object(42, owner);
        let mut server: Option<Server> = None;
        let msg = GameMessage::DestroyGameObject {
            network_object_id: 42,
            from_client_id: attacker,
        };
        handle_destroy(
            &mut server,
            &mut scene,
            MessageSender::Server,
            42,
            attacker,
            &msg,
        );
        scene.flush_deletes();

        assert!(scene.entry(go).is_some(), "non-owner destroyed the object");
    }

    #[test]
    fn destroy_is_applied_before_a_later_ownership_transfer() {
        let (owner, other) = (1u64, 2u64);
        let mut game = test_game_context();
        let go = {
            let scene = game.scenes.current_scene_mut();
            let go = scene.create(None, None);
            scene.add_component(
                go,
                ComponentNetworkObject {
                    id: 42,
                    owner_id: owner,
                },
            );
            go
        };

        // The owner destroys the object, then transfers it, and both land in
        // the same tick.
        let queue = &mut game.resources.network_mut().queue;
        queue.queue_message(
            MessageSender::Server,
            GameMessage::DestroyGameObject {
                network_object_id: 42,
                from_client_id: owner,
            },
        );
        queue.queue_message(
            MessageSender::Server,
            GameMessage::TransferOwnership {
                network_object_id: 42,
                from_client_id: owner,
                to_client_id: other,
            },
        );

        let GameContext {
            assets,
            scenes,
            resources,
        } = &mut game;
        resources
            .network_mut()
            .update_scene(scenes.current_scene_mut(), &assets.lock_read());
        let scene = game.scenes.current_scene_mut();
        scene.flush_deletes();

        assert!(
            scene.entry(go).is_none(),
            "the transfer was applied before the earlier destroy"
        );
    }

    #[test]
    fn destroy_game_object_refuses_non_owner() {
        let (mut scene, go) = scene_with_object(42, 1);
        let mut network = Network::new(60.0);
        network.local_id = Some(2);

        assert!(!network.destroy_game_object(&mut scene, go));
        scene.flush_deletes();
        assert!(scene.entry(go).is_some());
    }

    // --- Integration tests ---

    #[test]
    fn server_and_client_connect() {
        let mut net = TestNetwork::new();
        assert!(net.connect(), "client failed to connect to server");
        assert!(net.clients[0].client.is_connected());
    }

    #[test]
    fn server_detects_client_connection() {
        let mut net = TestNetwork::new();
        assert!(net.connect());

        // Pump once more so server processes the connection event
        net.pump();

        let server = net.server.server.as_ref().unwrap();
        assert!(!server.client_ids().is_empty());
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
            .send_message(client_id, &GameMessage::SyncTime { current_time: 99.0 })
            .unwrap();

        // Pump to deliver
        for _ in 0..10 {
            net.pump();
            std::thread::sleep(Duration::from_millis(1));
        }

        // Check client received it
        let mut received_time = None;
        struct TimeCapture;
        impl MessageHandler<Option<f64>, GameMessage> for TimeCapture {
            fn handle_message(
                &mut self,
                out: &mut Option<f64>,
                _sender: MessageSender,
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
        net.clients[0]
            .queue
            .receive_messages(None, &mut received_time, &mut TimeCapture);
        assert_eq!(received_time, Some(99.0));
    }

    #[test]
    fn three_peers_connect_to_one_server() {
        let mut net = TestNetwork::with_clients(2);
        assert!(net.connect(), "clients failed to connect to server");
        net.pump_until_drained();

        assert_eq!(net.server.server.as_ref().unwrap().client_ids().len(), 2);
    }

    #[test]
    fn server_broadcast_reaches_both_clients() {
        let mut net = TestNetwork::with_clients(2);
        assert!(net.connect());
        net.pump_until_drained();

        net.server
            .server
            .as_mut()
            .unwrap()
            .broadcast_message(&GameMessage::SyncTime { current_time: 7.0 })
            .unwrap();
        net.pump_until_drained();

        struct TimeCapture;
        impl MessageHandler<Option<f64>, GameMessage> for TimeCapture {
            fn handle_message(
                &mut self,
                out: &mut Option<f64>,
                _sender: MessageSender,
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
        for (i, client) in net.clients.iter_mut().enumerate() {
            let mut received = None;
            client
                .queue
                .receive_messages(None, &mut received, &mut TimeCapture);
            assert_eq!(received, Some(7.0), "client {i} missed the broadcast");
        }
    }

    /// Server-side scene holding one network object owned by `owner_id`.
    fn scene_with_object(network_object_id: u64, owner_id: u64) -> (Scene, GameObject) {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.add_component(
            go,
            ComponentNetworkObject {
                id: network_object_id,
                owner_id,
            },
        );
        (scene, go)
    }

    /// Applies the server's queued messages to `scene`, as the host would.
    fn apply_server_queue(net: &mut TestNetwork, scene: &mut Scene) {
        let Network {
            client,
            server,
            queue,
            local_id,
            ..
        } = &mut net.server;
        let mut scene = scene;
        queue.receive_messages(
            None,
            &mut (client, server, &mut scene, local_id),
            &mut NetworkSceneSync,
        );
    }

    /// Counts the `TransferOwnership` messages waiting in `network`'s queue.
    fn count_transfers(network: &mut Network) -> usize {
        struct TransferCounter;
        impl MessageHandler<usize, GameMessage> for TransferCounter {
            fn handle_message(
                &mut self,
                count: &mut usize,
                _sender: MessageSender,
                message: &GameMessage,
            ) -> MessageHandlerResult {
                if let GameMessage::TransferOwnership { .. } = message {
                    *count += 1;
                    MessageHandlerResult::Consume
                } else {
                    MessageHandlerResult::Ignore
                }
            }
        }
        let mut count = 0;
        network
            .queue
            .receive_messages(None, &mut count, &mut TransferCounter);
        count
    }

    #[test]
    fn server_rejects_spoofed_sender() {
        let mut net = TestNetwork::with_clients(2);
        assert!(net.connect());
        net.pump_until_drained();

        let attacker_id = net.clients[0].client.client_id().unwrap();
        let victim_id = net.clients[1].client.client_id().unwrap();
        let (mut scene, go) = scene_with_object(42, victim_id);

        // The attacker claims to be the victim to steal the victim's object.
        net.clients[0]
            .client
            .send_message(&GameMessage::TransferOwnership {
                network_object_id: 42,
                from_client_id: victim_id,
                to_client_id: attacker_id,
            })
            .unwrap();
        net.pump_until_drained();
        apply_server_queue(&mut net, &mut scene);
        net.pump_until_drained();

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(victim_id),
            "spoofed transfer changed ownership on the server"
        );
        assert_eq!(
            count_transfers(&mut net.clients[1]),
            0,
            "server relayed the spoofed transfer to the victim"
        );
    }

    #[test]
    fn server_accepts_genuine_sender() {
        let mut net = TestNetwork::with_clients(2);
        assert!(net.connect());
        net.pump_until_drained();

        let owner_id = net.clients[0].client.client_id().unwrap();
        let receiver_id = net.clients[1].client.client_id().unwrap();
        let (mut scene, go) = scene_with_object(42, owner_id);

        net.clients[0]
            .client
            .send_message(&GameMessage::TransferOwnership {
                network_object_id: 42,
                from_client_id: owner_id,
                to_client_id: receiver_id,
            })
            .unwrap();
        net.pump_until_drained();
        apply_server_queue(&mut net, &mut scene);
        net.pump_until_drained();

        assert_eq!(
            scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id),
            Some(receiver_id)
        );
        assert_eq!(count_transfers(&mut net.clients[1]), 1);
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
            .send_message(client_net_id, &GameMessage::SyncTime { current_time: 42.0 })
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
        impl MessageHandler<Option<f64>, GameMessage> for Capture {
            fn handle_message(
                &mut self,
                out: &mut Option<f64>,
                _sender: MessageSender,
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
            .receive_messages(None, &mut received_time, &mut Capture);
        assert_eq!(received_time, Some(42.0));
    }

    #[test]
    fn remote_prefab_spawn_is_owned_by_sender() {
        use crate::scene::Prefab;

        let mut harness = TestHarness::new(2);
        assert!(harness.wait_connected(), "not all clients connected");

        // Every peer registers the same prefab under the same name, so the
        // spawn's prefab id resolves on each of them.
        let prefab_refs: Vec<_> = harness
            .contexts
            .iter_mut()
            .map(|ctx| {
                let scene = ctx.scenes.current_scene_mut();
                let root = scene.create(None, None);
                scene.add_component(root, ComponentNetworkObject::default());
                let prefab: Prefab = scene.create_prefab(root);
                scene.delete(root);
                ctx.assets
                    .registries
                    .assets
                    .read()
                    .create("net_spawn_test_prefab".to_string(), prefab)
                    .expect("failed to register prefab")
            })
            .collect();

        // Wait until both clients know their own id.
        for _ in 0..200 {
            harness.pump();
            if (0..harness.client_count())
                .all(|i| harness.client(i).resources.network().local_id.is_some())
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let sender_id = harness
            .client(0)
            .resources
            .network()
            .local_id
            .expect("sender has no local id");
        let receiver_id = harness
            .client(1)
            .resources
            .network()
            .local_id
            .expect("receiver has no local id");
        assert_ne!(sender_id, receiver_id);

        let sender = harness.client_mut(0);
        let sender_scene = sender.scenes.current_scene_mut();
        let spawned = sender
            .resources
            .network_mut()
            .instantiate_prefab(sender_scene, prefab_refs[1].clone())
            .expect("sender failed to spawn prefab");
        let network_object_id = sender
            .scenes
            .current_scene()
            .read_component::<ComponentNetworkObject, _, _>(spawned, |c| c.id)
            .expect("spawned prefab has no network object");

        let find_owner = |scene: &crate::scene::Scene| {
            scene.objects().find_map(|go| {
                scene
                    .read_component::<ComponentNetworkObject, _, _>(go, |c| {
                        (c.id == network_object_id).then_some(c.owner_id)
                    })
                    .flatten()
            })
        };

        let mut receiver_owner = None;
        for _ in 0..400 {
            harness.pump();
            receiver_owner = find_owner(harness.client(1).scenes.current_scene());
            if receiver_owner.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(
            receiver_owner,
            Some(sender_id),
            "receiving peer must record the sending client as owner"
        );
        assert_eq!(
            find_owner(harness.client(0).scenes.current_scene()),
            Some(sender_id)
        );
    }

    /// Registers the same one-object network prefab on every peer under
    /// `name`, so a spawn's prefab id resolves on each of them, and waits
    /// until every client knows its own id.
    fn setup_spawn_harness(name: &str) -> (TestHarness, Vec<Ref<Prefab>>) {
        let mut harness = TestHarness::new(2);
        assert!(harness.wait_connected(), "not all clients connected");

        let prefab_refs = harness
            .contexts
            .iter_mut()
            .map(|ctx| {
                let scene = ctx.scenes.current_scene_mut();
                let root = scene.create(None, None);
                scene.add_component(root, ComponentNetworkObject::default());
                let prefab: Prefab = scene.create_prefab(root);
                scene.delete(root);
                scene.flush_deletes();
                ctx.assets
                    .registries
                    .assets
                    .read()
                    .create(name.to_string(), prefab)
                    .expect("failed to register prefab")
            })
            .collect();

        for _ in 0..200 {
            harness.pump();
            if (0..harness.client_count())
                .all(|i| harness.client(i).resources.network().local_id.is_some())
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        (harness, prefab_refs)
    }

    /// Spawns `prefab_ref` from client `index` and returns the root with its
    /// network object id.
    fn spawn_from_client(
        harness: &mut TestHarness,
        index: usize,
        prefab_ref: Ref<Prefab>,
    ) -> (GameObject, u64) {
        let ctx = harness.client_mut(index);
        let scene = ctx.scenes.current_scene_mut();
        let spawned = ctx
            .resources
            .network_mut()
            .instantiate_prefab(scene, prefab_ref)
            .expect("failed to spawn prefab");
        let network_object_id = ctx
            .scenes
            .current_scene()
            .read_component::<ComponentNetworkObject, _, _>(spawned, |c| c.id)
            .expect("spawned prefab has no network object");
        (spawned, network_object_id)
    }

    /// Returns whether `scene` holds network object `network_object_id`.
    fn has_network_object(scene: &Scene, network_object_id: u64) -> bool {
        scene.objects().any(|go| {
            scene
                .read_component::<ComponentNetworkObject, _, _>(go, |c| c.id == network_object_id)
                .unwrap_or(false)
        })
    }

    /// Pumps the harness, flushing deletions on every peer as the game loop
    /// does, until `done` holds or the attempts run out.
    fn pump_until(harness: &mut TestHarness, mut done: impl FnMut(&TestHarness) -> bool) -> bool {
        for _ in 0..400 {
            harness.pump();
            for ctx in &mut harness.contexts {
                ctx.scenes.current_scene_mut().flush_deletes();
            }
            if done(harness) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn destroyed_object_is_removed_on_all_peers() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_destroy_test_prefab");
        let (spawned, network_object_id) =
            spawn_from_client(&mut harness, 0, prefab_refs[1].clone());

        assert!(
            pump_until(&mut harness, |h| has_network_object(
                h.client(1).scenes.current_scene(),
                network_object_id
            )),
            "spawn never reached the other client"
        );

        // The owner destroys its object locally and tells the other peers,
        // as `Network::destroy_game_object` does.
        let owner = harness.client_mut(0);
        owner.scenes.current_scene_mut().delete(spawned);
        let network = owner.resources.network_mut();
        let from_client_id = network.local_id.expect("owner has no local id");
        network
            .client
            .send_message(&GameMessage::DestroyGameObject {
                network_object_id,
                from_client_id,
            })
            .expect("failed to send DestroyGameObject");

        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                !has_network_object(ctx.scenes.current_scene(), network_object_id)
            })),
            "destroyed object lingers on a peer"
        );
    }

    #[test]
    fn destroy_right_after_spawn_leaves_no_ghost() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_destroy_test_prefab");

        // Spawn and destroy before any peer pumps, so the server receives
        // both in the same tick.
        let (spawned, destroyed_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        let owner = harness.client_mut(0);
        let scene = owner.scenes.current_scene_mut();
        assert!(owner
            .resources
            .network_mut()
            .destroy_game_object(scene, spawned));

        // A later spawn travels behind the destroy on the ordered channel, so
        // once it lands the destroy has been handled everywhere.
        let (_, marker_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        assert!(
            pump_until(&mut harness, |h| has_network_object(
                h.client(1).scenes.current_scene(),
                marker_id
            )),
            "marker spawn never reached the other client"
        );

        for (i, ctx) in harness.contexts.iter().enumerate() {
            assert!(
                !has_network_object(ctx.scenes.current_scene(), destroyed_id),
                "peer {i} kept a ghost of the destroyed object"
            );
        }
    }
}
