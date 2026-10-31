#[cfg(test)]
mod tests {
    use crate::component::{ComponentCamera, ComponentEventContext};
    use crate::context::GameContext;
    use crate::core::Ref;
    use crate::net::sync::{SynchronizationOptions, Synchronized};
    use crate::net::{
        handle_component_enabled, handle_destroy, network_id_range, Client, ComponentNetworkObject,
        GameChannel, GameMessage, MessageHandler, MessageHandlerResult, MessageQueue, MessageRoute,
        MessageSender, Network, NetworkIdAllocator, NetworkSceneSync, ReplicationRecord,
        RoutedMessage, Server, ServerEvent, SnapshotEntry, SnapshotPlan, SNAPSHOT_CHUNK_BYTES,
    };
    use crate::scene::{ComponentEdit, GameObject, Prefab, Scene};
    use crate::test_support::NetworkTestHarness as TestHarness;
    use crate::test_support::{test_game_context, test_scene};
    use crate::utils::TypeUuid;
    use std::collections::{BTreeMap, HashSet};
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
    fn network_ids_are_unique_and_monotonic_within_the_assigned_range() {
        let mut ids = NetworkIdAllocator::default();
        assert_eq!(ids.allocate(), None, "no range assigned yet");

        ids.assign(7);
        let id1 = ids.allocate().unwrap();
        let id2 = ids.allocate().unwrap();
        let id3 = ids.allocate().unwrap();
        assert!(id1 < id2 && id2 < id3);
        assert!([id1, id2, id3].iter().all(|&id| network_id_range(id) == 7));
        assert_ne!(id1, 7 << 32, "the counter starts at 1");

        ids.assign(8);
        assert_eq!(ids.allocate(), Some((8 << 32) | 1));
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
            ..
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
        assert_eq!(GameMessage::PROTOCOL_ID, 0x8428_159d_3985_9fab);
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
                client_ids: Vec::new(),
                id_range: 1,
            }
            .channel(),
            GameChannel::ReliableOrdered
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

        // An enabled-state change must not be lost or overtake the spawn of
        // its object.
        assert_eq!(
            GameMessage::SetComponentEnabled {
                network_object_id: 7,
                from_client_id: 1,
                component_uuid: uuid::Uuid::nil(),
                enabled: false,
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
            ..
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
        setup_spawn_harness_with(name, |_, _| {})
    }

    /// [`setup_spawn_harness`] with a prefab root that `build` adds to.
    fn setup_spawn_harness_with(
        name: &str,
        build: impl Fn(&mut Scene, GameObject),
    ) -> (TestHarness, Vec<Ref<Prefab>>) {
        let mut harness = TestHarness::new(2);
        assert!(harness.wait_connected(), "not all clients connected");

        let prefab_refs = harness
            .contexts
            .iter_mut()
            .map(|ctx| {
                let scene = ctx.scenes.current_scene_mut();
                let root = scene.create(None, None);
                scene.add_component(root, ComponentNetworkObject::default());
                build(scene, root);
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

    /// Whether the object carrying network object `network_object_id` in
    /// `scene` holds its camera enabled, or `None` without such an object.
    fn camera_enabled(scene: &Scene, network_object_id: u64) -> Option<bool> {
        network_object(scene, network_object_id)
            .map(|go| scene.is_component_enabled::<ComponentCamera>(go))
    }

    /// The object carrying network object `network_object_id` in `scene`.
    fn network_object(scene: &Scene, network_object_id: u64) -> Option<GameObject> {
        scene.objects().find(|go| {
            scene
                .read_component::<ComponentNetworkObject, _, _>(*go, |c| c.id == network_object_id)
                .unwrap_or(false)
        })
    }

    #[test]
    fn component_enabled_change_reaches_every_peer_through_the_server() {
        let (mut harness, prefab_refs) =
            setup_spawn_harness_with("net_component_enabled_test_prefab", |scene, root| {
                scene.add_component(root, ComponentCamera::default());
            });
        let (spawned, network_object_id) =
            spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        assert!(
            pump_until(&mut harness, |h| camera_enabled(
                h.client(1).scenes.current_scene(),
                network_object_id
            ) == Some(true)),
            "spawn never reached the other client"
        );

        // Another peer may not change the owner's object.
        let other = harness.client_mut(1);
        let other_object = network_object(other.scenes.current_scene(), network_object_id)
            .expect("the other client holds the spawn");
        let scene = other.scenes.current_scene_mut();
        assert!(!other.resources.network_mut().set_component_enabled(
            scene,
            other_object,
            ComponentCamera::type_uuid(),
            false
        ));

        let owner = harness.client_mut(0);
        let scene = owner.scenes.current_scene_mut();
        assert!(owner.resources.network_mut().set_component_enabled(
            scene,
            spawned,
            ComponentCamera::type_uuid(),
            false
        ));
        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                camera_enabled(ctx.scenes.current_scene(), network_object_id) == Some(false)
            })),
            "a peer still holds the camera enabled"
        );
    }

    #[test]
    fn component_enabled_change_from_non_owner_is_rejected() {
        let (owner, attacker) = (1u64, 2u64);
        let (mut scene, go) = scene_with_object(42, owner);
        scene.add_component(go, ComponentCamera::default());
        let mut server: Option<Server> = None;
        let message = |from_client_id| GameMessage::SetComponentEnabled {
            network_object_id: 42,
            from_client_id,
            component_uuid: ComponentCamera::type_uuid(),
            enabled: false,
        };
        handle_component_enabled(
            &mut server,
            &mut scene,
            MessageSender::Server,
            &message(attacker),
        );
        assert!(scene.is_component_enabled::<ComponentCamera>(go));

        handle_component_enabled(
            &mut server,
            &mut scene,
            MessageSender::Server,
            &message(owner),
        );
        assert!(!scene.is_component_enabled::<ComponentCamera>(go));
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

    #[test]
    fn spawned_objects_are_indexed_on_every_peer() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_index_test_prefab");
        let (spawned, network_object_id) =
            spawn_from_client(&mut harness, 0, prefab_refs[1].clone());

        assert_eq!(
            harness
                .client(0)
                .scenes
                .current_scene()
                .indexed_network_object(network_object_id),
            Some(spawned),
            "local instantiate did not index the object"
        );

        assert!(
            pump_until(&mut harness, |h| h
                .client(1)
                .scenes
                .current_scene()
                .indexed_network_object(network_object_id)
                .is_some()),
            "remote spawn did not index the object"
        );
    }

    /// Returns the owner of network object `network_object_id` in `scene`.
    fn network_object_owner(scene: &Scene, network_object_id: u64) -> Option<u64> {
        scene.objects().find_map(|go| {
            scene
                .read_component::<ComponentNetworkObject, _, _>(go, |c| {
                    (c.id == network_object_id).then_some(c.owner_id)
                })
                .flatten()
        })
    }

    #[test]
    fn client_spawn_reaches_the_host_and_the_other_client() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_relay_spawn_test_prefab");
        let sender_id = harness.client(0).resources.network().local_id.unwrap();
        let (_, network_object_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());

        assert!(
            pump_until(&mut harness, |h| {
                [h.host(), h.client(1)].iter().all(|ctx| {
                    network_object_owner(ctx.scenes.current_scene(), network_object_id)
                        == Some(sender_id)
                })
            }),
            "the spawn did not reach both the host and the other client"
        );
    }

    /// Consumes component-sync messages and records the sender each claims.
    struct SyncCapture;
    impl MessageHandler<Vec<u64>, GameMessage> for SyncCapture {
        fn handle_message(
            &mut self,
            senders: &mut Vec<u64>,
            _: MessageSender,
            message: &GameMessage,
        ) -> MessageHandlerResult {
            match message {
                GameMessage::SyncComponent { from_client_id, .. } => {
                    senders.push(*from_client_id);
                    MessageHandlerResult::Consume
                }
                _ => MessageHandlerResult::Ignore,
            }
        }
    }

    /// Pumps the harness `rounds` times and returns, per peer, the claimed
    /// senders of the component-sync messages each received for `route`.
    fn collect_syncs(
        harness: &mut TestHarness,
        route: MessageRoute,
        rounds: usize,
    ) -> Vec<Vec<u64>> {
        let mut received = vec![Vec::new(); harness.contexts.len()];
        for _ in 0..rounds {
            harness.pump();
            for (ctx, senders) in harness.contexts.iter_mut().zip(&mut received) {
                ctx.resources.network_mut().queue.receive_messages(
                    Some(route),
                    senders,
                    &mut SyncCapture,
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        received
    }

    /// Spawns a network object from client 0, waits until every peer holds
    /// it, and returns the route of its transform sync with the owner's id.
    fn spawn_synced_object(name: &str) -> (TestHarness, MessageRoute, u64) {
        let (mut harness, prefab_refs) = setup_spawn_harness(name);
        let owner_id = harness.client(0).resources.network().local_id.unwrap();
        let (_, network_object_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                has_network_object(ctx.scenes.current_scene(), network_object_id)
            })),
            "the spawn did not reach every peer"
        );
        let route = MessageRoute {
            network_object_id,
            component_uuid: uuid::Uuid::from_u128(0x5eed),
        };
        (harness, route, owner_id)
    }

    /// Sends a component sync for `route` from client `index`, as
    /// `Synchronized` does for an object the client owns.
    fn send_sync_from_client(harness: &mut TestHarness, index: usize, route: MessageRoute) {
        let network = harness.client_mut(index).resources.network_mut();
        let from_client_id = network.local_id.unwrap();
        network
            .client
            .send_message(&GameMessage::SyncComponent {
                time: 0.0,
                network_object_id: route.network_object_id,
                from_client_id,
                component_uuid: route.component_uuid,
                data: Vec::new(),
            })
            .unwrap();
    }

    #[test]
    fn client_component_sync_reaches_the_other_client_through_the_server() {
        let (mut harness, route, owner_id) = spawn_synced_object("net_relay_sync_test_prefab");

        send_sync_from_client(&mut harness, 0, route);
        let received = collect_syncs(&mut harness, route, 60);

        assert_eq!(received[0], vec![owner_id], "the host missed the sync");
        assert!(
            received[1].is_empty(),
            "the server echoed the sync to its sender"
        );
        assert_eq!(
            received[2],
            vec![owner_id],
            "the other client missed the sync"
        );
    }

    #[test]
    fn non_owner_component_sync_is_not_relayed() {
        let (mut harness, route, _) = spawn_synced_object("net_relay_non_owner_test_prefab");

        send_sync_from_client(&mut harness, 1, route);
        let received = collect_syncs(&mut harness, route, 60);

        assert!(
            received[1].is_empty(),
            "the server relayed a sync for an object its sender does not own"
        );
    }

    #[test]
    fn listen_server_host_spawn_reaches_every_client() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_relay_host_spawn_test_prefab");
        let host = harness.host_mut();
        let host_id = host.resources.network().local_id.unwrap();
        let scene = host.scenes.current_scene_mut();
        let spawned = host
            .resources
            .network_mut()
            .instantiate_prefab(scene, prefab_refs[0].clone())
            .expect("the host failed to spawn the prefab");
        let network_object_id = host
            .scenes
            .current_scene()
            .read_component::<ComponentNetworkObject, _, _>(spawned, |c| c.id)
            .unwrap();

        assert!(
            pump_until(&mut harness, |h| (0..h.client_count()).all(|i| {
                network_object_owner(h.client(i).scenes.current_scene(), network_object_id)
                    == Some(host_id)
            })),
            "the host's spawn did not reach every client"
        );
    }

    #[test]
    fn client_disconnect_reaches_the_remaining_clients() {
        let (mut harness, _) = setup_spawn_harness("net_relay_disconnect_test_prefab");
        let leaving_id = harness.client(1).resources.network().local_id.unwrap();
        assert!(
            pump_until(&mut harness, |h| h
                .client(0)
                .resources
                .network()
                .client
                .client_ids()
                .contains(&leaving_id)),
            "the remaining client never learned of the other client"
        );

        // The host's transport reports the departure as the server does.
        let GameContext {
            assets,
            scenes,
            resources,
            ..
        } = harness.host_mut();
        let network = resources.network_mut();
        network.queue.queue_message(
            MessageSender::Local,
            GameMessage::ServerEvent(ServerEvent::ClientDisconnected {
                client_id: leaving_id,
            }),
        );
        network.update_scene(scenes.current_scene_mut(), &assets.lock_read());

        assert!(
            pump_until(&mut harness, |h| !h
                .client(0)
                .resources
                .network()
                .client
                .client_ids()
                .contains(&leaving_id)),
            "the remaining client still lists the departed client"
        );
    }

    #[test]
    fn destroyed_object_leaves_the_index() {
        let (mut scene, go) = scene_with_object(42, 1);
        assert_eq!(scene.find_network_object(42), Some(go));

        let mut server: Option<Server> = None;
        let msg = GameMessage::DestroyGameObject {
            network_object_id: 42,
            from_client_id: 1,
        };
        handle_destroy(&mut server, &mut scene, MessageSender::Server, 42, 1, &msg);
        scene.flush_deletes();

        assert_eq!(scene.indexed_network_object(42), None);
        assert_eq!(scene.find_network_object(42), None);
    }

    fn set_network_id(instance: &mut dyn crate::component::Component, id: u64) {
        instance
            .as_reflect_mut()
            .downcast_mut::<ComponentNetworkObject>()
            .expect("instance should be a network object")
            .id = id;
    }

    #[test]
    fn type_erased_write_reindexes_network_object() {
        let (mut scene, go) = scene_with_object(42, 1);
        assert_eq!(scene.indexed_network_object(42), Some(go));

        // The undo and remote paths write through the registry prototype.
        let prototype = ComponentNetworkObject::default();
        scene
            .write_component_dyn(go, &prototype, |instance| set_network_id(instance, 43))
            .expect("the object should have a network object");

        assert_eq!(scene.indexed_network_object(42), None);
        assert_eq!(scene.indexed_network_object(43), Some(go));
    }

    #[test]
    fn inspector_edit_reindexes_network_object() {
        let (mut scene, go) = scene_with_object(42, 1);
        let prototype = ComponentNetworkObject::default();
        scene
            .edit_component_dyn(go, &prototype, |instance, _| ComponentEdit {
                value: set_network_id(instance, 43),
                written: true,
            })
            .expect("the object should have a network object");

        assert_eq!(scene.indexed_network_object(42), None);
        assert_eq!(scene.indexed_network_object(43), Some(go));
    }

    #[test]
    fn network_object_added_by_type_uuid_is_indexed() {
        let mut scene = test_scene();
        let go = scene.create(None, None);
        scene.bind_component_dyn(go, ComponentNetworkObject::type_uuid());
        assert_eq!(
            scene.indexed_network_object(ComponentNetworkObject::default().id),
            Some(go),
            "bind_component_dyn did not index the default instance"
        );
        scene
            .write_component::<ComponentNetworkObject, _>(go, |c| c.id = 42)
            .expect("bind_component_dyn did not add the component");

        assert_eq!(scene.indexed_network_object(42), Some(go));
        assert_eq!(
            scene.indexed_network_object(0),
            None,
            "old id still indexed"
        );

        assert!(scene.remove_component_dyn(go, ComponentNetworkObject::type_uuid()));
        assert_eq!(scene.indexed_network_object(42), None);
    }

    #[test]
    fn loaded_scene_indexes_network_objects() {
        let (scene, _) = scene_with_object(42, 1);
        let loaded = scene.clone();
        let go = loaded
            .objects()
            .find(|go| {
                loaded
                    .read_component::<ComponentNetworkObject, _, _>(*go, |_| ())
                    .is_some()
            })
            .expect("network object was not loaded");

        assert_eq!(loaded.indexed_network_object(42), Some(go));
    }

    #[test]
    fn index_keeps_one_entry_per_id_and_object() {
        let (mut scene, a) = scene_with_object(42, 1);

        // Rewriting the id through the scene drops the old id's entry.
        scene.write_component::<ComponentNetworkObject, _>(a, |c| c.id = 43);
        assert_eq!(scene.indexed_network_object(42), None);
        assert_eq!(scene.indexed_network_object(43), Some(a));

        // Two objects briefly claim one id: the last writer is indexed.
        let b = scene.create(None, None);
        scene.add_component(
            b,
            ComponentNetworkObject {
                id: 43,
                owner_id: 1,
            },
        );
        assert_eq!(scene.indexed_network_object(43), Some(b));
        scene.write_component::<ComponentNetworkObject, _>(a, |c| c.id = 43);
        assert_eq!(scene.indexed_network_object(43), Some(a));
    }

    /// Returns how many objects in `scene` carry network object
    /// `network_object_id`.
    fn count_network_object(scene: &Scene, network_object_id: u64) -> usize {
        scene
            .objects()
            .filter(|&go| {
                scene
                    .read_component::<ComponentNetworkObject, _, _>(go, |c| {
                        c.id == network_object_id
                    })
                    .unwrap_or(false)
            })
            .count()
    }

    /// Returns how many objects in `scene` carry a network object.
    fn count_network_objects(scene: &Scene) -> usize {
        scene
            .objects()
            .filter(|&go| {
                scene
                    .read_component::<ComponentNetworkObject, _, _>(go, |_| ())
                    .is_some()
            })
            .count()
    }

    /// Sends a hand-made `SpawnPrefab` from client `index`, as a buggy or
    /// hostile client could, without instantiating anything locally.
    fn send_spawn_from_client(
        harness: &mut TestHarness,
        index: usize,
        prefab_ref: &Ref<Prefab>,
        network_object_ids: std::collections::HashMap<u32, u64>,
    ) {
        let network = harness.client_mut(index).resources.network_mut();
        let from_client_id = network.local_id.unwrap();
        network
            .client
            .send_message(&GameMessage::SpawnPrefab {
                network_object_ids,
                from_client_id,
                prefab_id: prefab_ref.id(),
            })
            .unwrap();
    }

    /// Spawns a marker from client `index` and waits until every peer holds
    /// it. The marker travels behind earlier spawns on the ordered channel,
    /// so once it lands they have been handled everywhere.
    fn wait_for_marker_spawn(harness: &mut TestHarness, index: usize, prefab_ref: Ref<Prefab>) {
        let (_, marker_id) = spawn_from_client(harness, index, prefab_ref);
        assert!(
            pump_until(harness, |h| h.contexts.iter().all(|ctx| {
                has_network_object(ctx.scenes.current_scene(), marker_id)
            })),
            "marker spawn never reached every peer"
        );
    }

    #[test]
    fn client_spawn_reusing_another_peers_id_is_refused() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_collide_test_prefab");
        let owner_id = harness.client(0).resources.network().local_id.unwrap();
        let (_, network_object_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                has_network_object(ctx.scenes.current_scene(), network_object_id)
            })),
            "the spawn did not reach every peer"
        );

        // Client 1 claims client 0's object id for a spawn of its own.
        send_spawn_from_client(
            &mut harness,
            1,
            &prefab_refs[2],
            [(0, network_object_id)].into_iter().collect(),
        );
        wait_for_marker_spawn(&mut harness, 1, prefab_refs[2].clone());

        for (name, ctx) in [("host", harness.host()), ("client 0", harness.client(0))] {
            let scene = ctx.scenes.current_scene();
            assert_eq!(
                count_network_object(scene, network_object_id),
                1,
                "the colliding spawn added a second object with the id on the {name}"
            );
            assert_eq!(
                network_object_owner(scene, network_object_id),
                Some(owner_id),
                "the colliding spawn changed the object's owner on the {name}"
            );
            let indexed_owner = scene
                .indexed_network_object(network_object_id)
                .and_then(|go| {
                    scene.read_component::<ComponentNetworkObject, _, _>(go, |c| c.owner_id)
                });
            assert_eq!(
                indexed_owner,
                Some(owner_id),
                "the colliding spawn changed which object the id resolves to on the {name}"
            );
        }
    }

    #[test]
    fn client_spawn_into_another_peers_id_range_is_refused() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_range_test_prefab");
        let (_, network_object_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                has_network_object(ctx.scenes.current_scene(), network_object_id)
            })),
            "the spawn did not reach every peer"
        );

        // An id client 0 has not used yet, but that comes from its range.
        let reserved_id = network_object_id + 1000;
        send_spawn_from_client(
            &mut harness,
            1,
            &prefab_refs[2],
            [(0, reserved_id)].into_iter().collect(),
        );
        wait_for_marker_spawn(&mut harness, 1, prefab_refs[2].clone());

        for (name, ctx) in [("host", harness.host()), ("client 0", harness.client(0))] {
            assert!(
                !has_network_object(ctx.scenes.current_scene(), reserved_id),
                "the {name} accepted a spawn with an id reserved by another peer"
            );
        }
    }

    #[test]
    fn client_spawn_with_missing_or_duplicate_ids_is_refused() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_malformed_test_prefab");
        wait_for_marker_spawn(&mut harness, 1, prefab_refs[2].clone());
        let before: Vec<_> = [harness.host(), harness.client(0)]
            .iter()
            .map(|ctx| count_network_objects(ctx.scenes.current_scene()))
            .collect();

        // The prefab holds one network object: no id leaves it unnamed, and
        // two indices sharing one id would alias each other.
        send_spawn_from_client(&mut harness, 1, &prefab_refs[2], Default::default());
        let (_, own_id) = spawn_from_client(&mut harness, 1, prefab_refs[2].clone());
        send_spawn_from_client(
            &mut harness,
            1,
            &prefab_refs[2],
            [(0, own_id + 500), (1, own_id + 500)].into_iter().collect(),
        );
        wait_for_marker_spawn(&mut harness, 1, prefab_refs[2].clone());

        // Only the genuine spawn and the marker arrived.
        let after: Vec<_> = [harness.host(), harness.client(0)]
            .iter()
            .map(|ctx| count_network_objects(ctx.scenes.current_scene()))
            .collect();
        assert_eq!(after, before.iter().map(|n| n + 2).collect::<Vec<_>>());
    }

    #[test]
    fn concurrent_client_spawns_never_collide() {
        const SPAWNS: usize = 5;
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_concurrent_test_prefab");
        let host_id = harness.host().resources.network().local_id.unwrap();
        let host_object_id = {
            let host = harness.host_mut();
            let scene = host.scenes.current_scene_mut();
            let spawned = host
                .resources
                .network_mut()
                .instantiate_prefab(scene, prefab_refs[0].clone())
                .unwrap();
            host.scenes
                .current_scene()
                .read_component::<ComponentNetworkObject, _, _>(spawned, |c| c.id)
                .unwrap()
        };

        // Both clients spawn before any peer pumps, so the server receives
        // their spawns in the same tick.
        let mut spawned: Vec<(u64, u64)> = vec![(host_object_id, host_id)];
        for _ in 0..SPAWNS {
            for index in 0..2 {
                let owner_id = harness.client(index).resources.network().local_id.unwrap();
                let (_, id) =
                    spawn_from_client(&mut harness, index, prefab_refs[index + 1].clone());
                spawned.push((id, owner_id));
            }
        }

        let ids: HashSet<_> = spawned.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), spawned.len(), "two spawns chose the same id");
        assert!(
            pump_until(&mut harness, |h| h.contexts.iter().all(|ctx| {
                spawned.iter().all(|&(id, owner_id)| {
                    network_object_owner(ctx.scenes.current_scene(), id) == Some(owner_id)
                })
            })),
            "a spawn did not reach every peer with its owner"
        );
        for ctx in &harness.contexts {
            for &(id, _) in &spawned {
                assert_eq!(count_network_object(ctx.scenes.current_scene(), id), 1);
            }
        }

        // Each peer allocates from its own range, whatever the others pick.
        let mut ranges: std::collections::HashMap<u64, HashSet<u64>> = Default::default();
        for &(id, owner_id) in &spawned {
            ranges.entry(owner_id).or_default().insert(id >> 32);
        }
        assert!(ranges.values().all(|r| r.len() == 1));
        let distinct: HashSet<_> = ranges.values().flatten().collect();
        assert_eq!(distinct.len(), 3, "two peers share an id range");
    }

    #[test]
    fn refused_spawn_does_not_shadow_an_existing_id_before_it_is_deleted() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_shadow_test_prefab");
        let owner_id = harness.client(0).resources.network().local_id.unwrap();

        // An object authored into the host's scene shares the id the prefab
        // itself carries.
        let host_scene = harness.host_mut().scenes.current_scene_mut();
        let authored = host_scene.create(None, None);
        host_scene.add_component(authored, ComponentNetworkObject { id: 0, owner_id });
        let objects_before = host_scene.objects().count();

        // No ids: the host instantiates the prefab, then refuses the spawn.
        // Deletion is deferred, so the refused instance lives until the next
        // flush, which this loop never runs.
        send_spawn_from_client(&mut harness, 1, &prefab_refs[2], Default::default());
        let mut instantiated = false;
        for _ in 0..400 {
            harness.pump();
            if harness.host().scenes.current_scene().objects().count() > objects_before {
                instantiated = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(instantiated, "the host never handled the spawn");

        assert_eq!(
            harness
                .host_mut()
                .scenes
                .current_scene_mut()
                .find_network_object(0),
            Some(authored),
            "the refused instance took over the authored object's id"
        );
    }

    #[test]
    fn index_restores_an_id_once_its_later_claimant_lets_go() {
        let (mut scene, live) = scene_with_object(42, 1);

        // A prefab made from the live object carries its id, so its instance
        // briefly claims that id too.
        let prefab = scene.create_prefab(live);
        let instance = scene.instantiate_prefab(&prefab, None).unwrap();
        assert_eq!(scene.indexed_network_object(42), Some(instance));

        // Once the instance takes another id, or drops its network object,
        // the id resolves to the live object again.
        scene.write_component::<ComponentNetworkObject, _>(instance, |c| c.id = 43);
        assert_eq!(scene.indexed_network_object(42), Some(live));
        let other = scene.instantiate_prefab(&prefab, None).unwrap();
        scene.remove_component::<ComponentNetworkObject>(other);
        assert_eq!(scene.indexed_network_object(42), Some(live));
        assert_eq!(scene.indexed_network_object(43), Some(instance));
    }

    #[test]
    fn relayed_spawn_without_ids_is_refused_by_receiving_clients() {
        let (mut harness, prefab_refs) = setup_spawn_harness("net_spawn_client_only_marker_prefab");
        // Only the clients hold this prefab, so the host relays spawns of it
        // without instantiating them.
        let client_only: Vec<_> = harness.contexts[1..]
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
                    .create("net_spawn_client_only_test_prefab".to_string(), prefab)
                    .expect("failed to register prefab")
            })
            .collect();
        let before = count_network_objects(harness.client(0).scenes.current_scene());

        send_spawn_from_client(&mut harness, 1, &client_only[1], Default::default());
        wait_for_marker_spawn(&mut harness, 1, prefab_refs[2].clone());

        // Only the marker arrived.
        assert_eq!(
            count_network_objects(harness.client(0).scenes.current_scene()),
            before + 1,
            "client 0 instantiated a spawn whose objects came without ids"
        );
    }

    // --- Join snapshot ---

    #[test]
    fn join_snapshot_messages_travel_ordered_and_only_from_the_server() {
        let messages = [
            GameMessage::SelfConnected {
                client_ids: Vec::new(),
                id_range: 1,
            },
            GameMessage::JoinSnapshot {
                entries: Vec::new(),
            },
            GameMessage::SnapshotComponent {
                network_object_id: 7,
                component_uuid: uuid::Uuid::nil(),
                data: Vec::new(),
            },
        ];
        for message in &messages {
            assert_eq!(message.channel(), GameChannel::ReliableOrdered);
            assert!(
                message.check_client_origin(1).is_err(),
                "a client may not send {message:?}"
            );
        }
        assert_eq!(
            messages[2].route(),
            Some(MessageRoute {
                network_object_id: 7,
                component_uuid: uuid::Uuid::nil(),
            }),
            "a snapshot payload goes to its synchronized component"
        );
    }

    #[test]
    fn replication_record_orders_the_snapshot() {
        let (spawner, receiver) = (1u64 << 32 | 5, 9u64);
        let component = uuid::Uuid::from_u128(0xc0);
        let (root, child) = ((1u64 << 32) | 1, (1u64 << 32) | 2);
        let ids = std::collections::HashMap::from([(0u32, root), (1u32, child)]);
        let prefab_id = uuid::Uuid::from_u128(0xfab);

        let mut record = ReplicationRecord::default();
        record.spawn(spawner, prefab_id, &ids);
        record.component_sync(child, component, &[1]);
        record.destroy(child, false);
        record.destroy(7, true);
        // An id no peer holds leaves no destroy behind.
        record.destroy(99, false);
        record.transfer(root, receiver);
        record.transfer(8, receiver);
        record.set_component_enabled(root, component, false);
        record.component_sync(root, component, &[1, 2]);
        record.component_sync(root, component, &[3]);

        assert_eq!(
            record.entries(),
            vec![
                SnapshotEntry::Destroy {
                    network_object_id: 7
                },
                SnapshotEntry::Spawn {
                    prefab_id,
                    network_object_ids: ids.clone(),
                    owner_id: spawner,
                },
                SnapshotEntry::Destroy {
                    network_object_id: child
                },
                SnapshotEntry::Owner {
                    network_object_id: 8,
                    owner_id: receiver,
                },
                SnapshotEntry::Owner {
                    network_object_id: root,
                    owner_id: receiver,
                },
                SnapshotEntry::ComponentEnabled {
                    network_object_id: root,
                    component_uuid: component,
                    enabled: false,
                },
            ]
        );
        let messages = record.messages(SNAPSHOT_CHUNK_BYTES);
        assert!(
            matches!(
                messages.last(),
                Some(GameMessage::SnapshotComponent { network_object_id, data, .. })
                    if *network_object_id == root && data == &[3]
            ),
            "the snapshot ends with the latest payload of each component, got {messages:?}"
        );
        assert_eq!(messages.len(), 2, "the destroyed child kept its payload");

        // Destroying the last object of a spawn drops it altogether.
        record.destroy(root, false);
        assert_eq!(
            record.entries(),
            vec![
                SnapshotEntry::Destroy {
                    network_object_id: 7
                },
                SnapshotEntry::Owner {
                    network_object_id: 8,
                    owner_id: receiver,
                },
            ]
        );
        assert!(!record.is_spawned(root));
    }

    #[test]
    fn join_snapshot_is_split_into_chunks_and_cut_to_its_budget() {
        let mut record = ReplicationRecord::default();
        let range = 1u64 << 32;
        for i in 1..=2000u64 {
            let ids = std::collections::HashMap::from([(0u32, range | i)]);
            record.spawn(1, uuid::Uuid::from_u128(u128::from(i)), &ids);
        }
        record.component_sync(range | 1, uuid::Uuid::nil(), &vec![0; 64 * 1024]);
        let messages = record.messages(SNAPSHOT_CHUNK_BYTES);

        let mut chunks = 0;
        let mut entries = Vec::new();
        for message in &messages {
            let size = bincode::serde::encode_to_vec(message, bincode::config::standard())
                .unwrap()
                .len();
            if let GameMessage::JoinSnapshot { entries: chunk } = message {
                chunks += 1;
                // The message adds its variant tag and length to the entries.
                assert!(
                    size <= SNAPSHOT_CHUNK_BYTES + 16,
                    "a {size}-byte chunk exceeds the chunk size"
                );
                entries.extend(chunk.iter().cloned());
            }
        }
        assert!(chunks > 1, "the snapshot was not split");
        assert_eq!(
            entries,
            record.entries(),
            "chunking lost or reordered entries"
        );

        let plan = SnapshotPlan::new(&messages, usize::MAX);
        assert!(!plan.is_truncated());
        assert_eq!(plan.messages.len(), messages.len());

        // The large payload is the tail; a budget that misses it cuts only it.
        let budget = plan.total_bytes - 1024;
        let plan = SnapshotPlan::new(&messages, budget);
        assert!(plan.is_truncated());
        assert!(plan.sent_bytes <= budget);
        assert_eq!(plan.messages.len(), messages.len() - 1);
    }

    /// Network object id of a scene-file object, which every peer loads.
    const SCENE_DESTROYED: u64 = 7;
    const SCENE_TRANSFERRED: u64 = 8;
    const SCENE_TOGGLED: u64 = 9;

    /// Component id of the [`Synchronized`] value the join tests sync.
    const SYNCED_COMPONENT: uuid::Uuid = uuid::Uuid::from_u128(0x5eed_0437);

    /// Prefab built by the join tests: a network object with a camera to
    /// toggle.
    fn add_camera(scene: &mut Scene, root: GameObject) {
        scene.add_component(root, ComponentCamera::default());
    }

    /// Registers the join tests' prefab on `ctx` under `name`, as
    /// [`setup_spawn_harness_with`] does on the peers it creates.
    fn register_camera_prefab(ctx: &mut GameContext, name: &str) -> Ref<Prefab> {
        let scene = ctx.scenes.current_scene_mut();
        let root = scene.create(None, None);
        scene.add_component(root, ComponentNetworkObject::default());
        add_camera(scene, root);
        let prefab: Prefab = scene.create_prefab(root);
        scene.delete(root);
        scene.flush_deletes();
        ctx.assets
            .registries
            .assets
            .read()
            .create(name.to_string(), prefab)
            .expect("failed to register prefab")
    }

    /// Adds the scene-file network objects `(id, owner)` to `ctx`'s scene,
    /// each with a camera, as loading the same scene file on every peer does.
    fn add_scene_objects(ctx: &mut GameContext, objects: &[(u64, u64)]) {
        let scene = ctx.scenes.current_scene_mut();
        for &(id, owner_id) in objects {
            let go = scene.create(None, None);
            scene.add_component(go, ComponentNetworkObject { id, owner_id });
            scene.add_component(go, ComponentCamera::default());
        }
    }

    /// Replicated state of `scene`: the owner of each network object and
    /// whether its camera is enabled.
    fn replicated_state(scene: &Scene) -> BTreeMap<u64, (u64, bool)> {
        scene
            .objects()
            .filter_map(|go| {
                let (id, owner_id) = scene
                    .read_component::<ComponentNetworkObject, _, _>(go, |c| (c.id, c.owner_id))?;
                Some((
                    id,
                    (owner_id, scene.is_component_enabled::<ComponentCamera>(go)),
                ))
            })
            .collect()
    }

    /// Whether every peer of `harness` holds the same replicated state.
    fn states_agree(harness: &TestHarness) -> bool {
        let host = replicated_state(harness.host().scenes.current_scene());
        harness.contexts[1..]
            .iter()
            .all(|ctx| replicated_state(ctx.scenes.current_scene()) == host)
    }

    /// Runs `sync` for network object `network_object_id` on `ctx` as its
    /// synchronized component does each update, with `value` as the local
    /// value, and returns the value it holds for a remote owner.
    fn run_sync(
        ctx: &mut GameContext,
        sync: &mut Synchronized<f32>,
        network_object_id: u64,
        value: f32,
    ) -> Option<f32> {
        let GameContext {
            assets,
            scenes,
            resources,
            ..
        } = ctx;
        let assets = assets.lock_read();
        let scene = scenes.current_scene_mut();
        let game_object = network_object(scene, network_object_id)?;
        let mut event_ctx = ComponentEventContext {
            registries: &assets.registries,
            scene,
            game_object,
        };
        sync.update(&mut event_ctx, resources, |_| value)
    }

    fn synced_value() -> Synchronized<f32> {
        Synchronized::new(
            SynchronizationOptions::builder()
                .interpolate(false)
                .component_uuid(SYNCED_COMPONENT)
                .build(),
        )
    }

    /// Records, without consuming it, whether a join snapshot reached a peer.
    struct SnapshotSeen;
    impl MessageHandler<bool, GameMessage> for SnapshotSeen {
        fn handle_message(
            &mut self,
            seen: &mut bool,
            _: MessageSender,
            message: &GameMessage,
        ) -> MessageHandlerResult {
            if matches!(message, GameMessage::JoinSnapshot { .. }) {
                *seen = true;
            }
            MessageHandlerResult::Ignore
        }
    }

    /// Pumps the harness once and flushes deletions on every peer, as the
    /// game loop does.
    fn pump_and_flush(harness: &mut TestHarness) {
        harness.pump();
        for ctx in &mut harness.contexts {
            ctx.scenes.current_scene_mut().flush_deletes();
        }
    }

    #[test]
    fn late_joiner_receives_the_replicated_state() {
        let name = "net_join_snapshot_test_prefab";
        let (mut harness, prefab_refs) = setup_spawn_harness_with(name, add_camera);
        let host_id = harness.host().resources.network().local_id.unwrap();
        let first_id = harness.client(0).resources.network().local_id.unwrap();
        let second_id = harness.client(1).resources.network().local_id.unwrap();
        let scene_objects = [
            (SCENE_DESTROYED, host_id),
            (SCENE_TRANSFERRED, first_id),
            (SCENE_TOGGLED, first_id),
        ];
        for ctx in &mut harness.contexts {
            add_scene_objects(ctx, &scene_objects);
        }

        // Scene-file objects: the host destroys one, client 0 hands one to
        // client 1 and disables the camera of another.
        let host = harness.host_mut();
        let scene = host.scenes.current_scene_mut();
        let destroyed = network_object(scene, SCENE_DESTROYED).unwrap();
        assert!(host
            .resources
            .network_mut()
            .destroy_game_object(scene, destroyed));
        let first = harness.client_mut(0);
        first
            .resources
            .network_mut()
            .client
            .send_message(&GameMessage::TransferOwnership {
                network_object_id: SCENE_TRANSFERRED,
                from_client_id: first_id,
                to_client_id: second_id,
            })
            .unwrap();
        let scene = first.scenes.current_scene_mut();
        let toggled = network_object(scene, SCENE_TOGGLED).unwrap();
        assert!(first.resources.network_mut().set_component_enabled(
            scene,
            toggled,
            ComponentCamera::type_uuid(),
            false
        ));

        // Runtime spawns from client 0 and from the host.
        let (_, kept_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        let (destroyed_spawn, destroyed_spawn_id) =
            spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        let (_, transferred_id) = spawn_from_client(&mut harness, 0, prefab_refs[1].clone());
        let host = harness.host_mut();
        let scene = host.scenes.current_scene_mut();
        let host_spawn = host
            .resources
            .network_mut()
            .instantiate_prefab(scene, prefab_refs[0].clone())
            .unwrap();
        let host_spawn_id = scene
            .read_component::<ComponentNetworkObject, _, _>(host_spawn, |c| c.id)
            .unwrap();
        assert!(
            pump_until(&mut harness, |h| [
                kept_id,
                destroyed_spawn_id,
                transferred_id
            ]
            .iter()
            .all(|&id| has_network_object(h.client(1).scenes.current_scene(), id))),
            "the spawns did not reach client 1"
        );

        // Changes to the spawned objects.
        let first = harness.client_mut(0);
        let scene = first.scenes.current_scene_mut();
        assert!(first
            .resources
            .network_mut()
            .destroy_game_object(scene, destroyed_spawn));
        let kept = network_object(scene, kept_id).unwrap();
        assert!(first.resources.network_mut().set_component_enabled(
            scene,
            kept,
            ComponentCamera::type_uuid(),
            false
        ));
        first
            .resources
            .network_mut()
            .client
            .send_message(&GameMessage::TransferOwnership {
                network_object_id: transferred_id,
                from_client_id: first_id,
                to_client_id: second_id,
            })
            .unwrap();
        let host = harness.host_mut();
        let scene = host.scenes.current_scene_mut();
        assert!(host.resources.network_mut().set_component_enabled(
            scene,
            host_spawn,
            ComponentCamera::type_uuid(),
            false
        ));

        // One sync each from client 0 and from the host, never resent.
        let mut client_sync = synced_value();
        run_sync(harness.client_mut(0), &mut client_sync, kept_id, 5.0);
        let mut host_sync = synced_value();
        run_sync(harness.host_mut(), &mut host_sync, host_spawn_id, 9.0);

        assert!(
            pump_until(&mut harness, |h| states_agree(h)
                && !has_network_object(
                    h.client(1).scenes.current_scene(),
                    destroyed_spawn_id
                )
                && network_object_owner(
                    h.client(1).scenes.current_scene(),
                    transferred_id
                ) == Some(second_id)),
            "the connected peers never agreed"
        );

        // A client joins, holding the same scene file and prefab.
        let late = harness.add_client();
        let late_ctx = harness.client_mut(late);
        register_camera_prefab(late_ctx, name);
        add_scene_objects(late_ctx, &scene_objects);
        // The loop below connects it too: its snapshot payloads wait for the
        // synchronized components only until the next update, as in a game
        // loop, so they must run from the first pump.
        let mut late_kept = synced_value();
        let mut late_host_spawn = synced_value();
        let mut values = (None, None);
        let mut host_saw_snapshot = false;
        let mut converged = false;
        for _ in 0..400 {
            pump_and_flush(&mut harness);
            harness
                .host_mut()
                .resources
                .network_mut()
                .queue
                .receive_messages(None, &mut host_saw_snapshot, &mut SnapshotSeen);
            let late_ctx = harness.client_mut(late);
            values.0 = run_sync(late_ctx, &mut late_kept, kept_id, 0.0).or(values.0);
            values.1 = run_sync(late_ctx, &mut late_host_spawn, host_spawn_id, 0.0).or(values.1);
            if states_agree(&harness) && values == (Some(5.0), Some(9.0)) {
                converged = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let host_state = replicated_state(harness.host().scenes.current_scene());
        let late_state = replicated_state(harness.client(late).scenes.current_scene());
        assert_eq!(late_state, host_state, "the late client's state differs");
        assert!(
            converged,
            "the late client never got the synced values: {values:?}"
        );
        assert!(!host_saw_snapshot, "the host sent itself a snapshot");

        // The comparison covers each kind of change.
        assert!(!late_state.contains_key(&SCENE_DESTROYED));
        assert!(!late_state.contains_key(&destroyed_spawn_id));
        assert_eq!(late_state[&SCENE_TRANSFERRED].0, second_id);
        assert!(!late_state[&SCENE_TOGGLED].1);
        assert_eq!(late_state[&kept_id], (first_id, false));
        assert_eq!(late_state[&transferred_id].0, second_id);
        assert_eq!(late_state[&host_spawn_id], (host_id, false));
    }

    #[test]
    fn join_snapshot_does_not_race_with_live_changes() {
        let name = "net_join_race_test_prefab";
        let (mut harness, prefab_refs) = setup_spawn_harness_with(name, add_camera);

        // Enough state that the snapshot spans several server ticks: renet
        // sends at most 60 000 bytes per connection per tick.
        let mut spawned = Vec::new();
        for _ in 0..40 {
            spawned.push(spawn_from_client(&mut harness, 0, prefab_refs[1].clone()));
        }
        assert!(
            pump_until(&mut harness, |h| spawned.iter().all(|(_, id)| {
                has_network_object(h.host().scenes.current_scene(), *id)
            })),
            "the spawns did not reach the host"
        );
        let first_id = harness.client(0).resources.network().local_id.unwrap();
        // Syncs are unreliable and renet drops what exceeds a tick's byte
        // budget, so they go a few per round until the host kept them all.
        let kept_payloads = |h: &TestHarness| {
            h.host()
                .resources
                .network()
                .server
                .as_ref()
                .unwrap()
                .record()
                .messages(SNAPSHOT_CHUNK_BYTES)
                .iter()
                .filter(|m| matches!(m, GameMessage::SnapshotComponent { .. }))
                .count()
        };
        for _ in 0..20 {
            if kept_payloads(&harness) == spawned.len() {
                break;
            }
            for batch in spawned.chunks(4) {
                for (_, id) in batch {
                    harness
                        .client_mut(0)
                        .resources
                        .network_mut()
                        .client
                        .send_message(&GameMessage::SyncComponent {
                            time: 0.0,
                            network_object_id: *id,
                            from_client_id: first_id,
                            component_uuid: SYNCED_COMPONENT,
                            data: vec![0; 8 * 1024],
                        })
                        .unwrap();
                }
                for _ in 0..3 {
                    pump_and_flush(&mut harness);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        assert_eq!(kept_payloads(&harness), spawned.len());
        let snapshot_bytes = SnapshotPlan::new(
            &harness
                .host()
                .resources
                .network()
                .server
                .as_ref()
                .unwrap()
                .record()
                .messages(SNAPSHOT_CHUNK_BYTES),
            usize::MAX,
        )
        .total_bytes;
        assert!(
            snapshot_bytes > 4 * 60_000,
            "the snapshot of {snapshot_bytes} bytes fits in a few ticks"
        );

        let late = harness.add_client();
        register_camera_prefab(harness.client_mut(late), name);

        // Client 0 keeps changing the scene, every round, while the late
        // client connects and receives its snapshot, and for a while after.
        let mut changes_after_id = 0;
        let mut round = 0;
        while changes_after_id < 30 {
            assert!(round < 1000, "the late client never learned its id");
            round += 1;
            let first = harness.client_mut(0);
            let scene = first.scenes.current_scene_mut();
            let network = first.resources.network_mut();
            let (go, id) = spawned.remove(0);
            match round % 3 {
                0 => {
                    network.destroy_game_object(scene, go);
                }
                1 => {
                    network.set_component_enabled(scene, go, ComponentCamera::type_uuid(), false);
                    spawned.push((go, id));
                }
                _ => {
                    let spawn = network
                        .instantiate_prefab(scene, prefab_refs[1].clone())
                        .unwrap();
                    let spawn_id = scene
                        .read_component::<ComponentNetworkObject, _, _>(spawn, |c| c.id)
                        .unwrap();
                    spawned.push((go, id));
                    spawned.push((spawn, spawn_id));
                }
            }
            pump_and_flush(&mut harness);
            if harness.client(late).resources.network().local_id.is_some() {
                changes_after_id += 1;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        assert!(
            pump_until(&mut harness, states_agree),
            "the late client's state differs: {:?} vs {:?}",
            replicated_state(harness.client(late).scenes.current_scene()),
            replicated_state(harness.host().scenes.current_scene())
        );
    }
}
