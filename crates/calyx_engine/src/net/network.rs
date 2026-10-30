use crate as calyx_engine;
use crate::context::ReadOnlyAssetContext;
use crate::core::{AbsoluteTime, Ref, Time, TimeType};
use crate::error::BoxedError;
use crate::net::client::Client;
use crate::net::server::Server;
use crate::net::{
    network_id_range, ComponentNetworkObject, GameMessage, MessageHandler, MessageHandlerResult,
    MessageQueue, MessageSender, NetworkObjectId, ServerEvent,
};
use crate::scene::{GameObject, Prefab, Scene};
use crate::utils::TypeUuid;
use calyx_engine_derive::Resource;
use renet::ClientId;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::net::SocketAddr;
use std::time::Duration;

/// High-level networking resource that owns the active client, optional host
/// server, protocol queue, and replication tick state.
#[derive(Resource, TypeUuid)]
#[uuid = "657869ba-a509-4340-89ae-f07d1eddf0db"]
pub struct Network {
    /// Local client connection and transport state.
    pub client: Client,
    /// Local host server when running in listen-server mode.
    pub server: Option<Server>,
    /// Shared queue of decoded protocol messages.
    pub queue: MessageQueue<GameMessage>,
    /// Client ID assigned to the local peer once connected or hosting.
    pub local_id: Option<ClientId>,
    tick_rate: TimeType,
    tick_period: TimeType,
    accumulated_time: TimeType,
    /// Seconds of unscaled time since the last time-sync broadcast.
    tick_sync_timer: TimeType,
}

impl Default for Network {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TICK_RATE_HZ)
    }
}

impl Network {
    const DEFAULT_TICK_RATE_HZ: f32 = 30.0;

    /// Creates a network resource with a fixed replication tick rate.
    pub fn new(tick_rate: f32) -> Self {
        assert!(tick_rate > 0.0);
        Self {
            client: Default::default(),
            server: None,
            queue: Default::default(),
            local_id: None,
            tick_rate,
            tick_period: 1.0 / tick_rate,
            accumulated_time: 0.0,
            tick_sync_timer: 0.0,
        }
    }

    /// Starts a local host server and assigns a local client ID, and the
    /// first network object id range, to the host peer.
    pub fn host(&mut self, socket_addr: SocketAddr) -> Result<(), BoxedError> {
        let mut server = Server::new(socket_addr)?;
        let local_id = Client::generate_client_id();
        let range = server
            .reserve_id_range(local_id)
            .ok_or("no network object id range left for the host")?;
        self.client.network_ids.assign(range);
        self.server = Some(server);
        self.local_id = Some(local_id);
        Ok(())
    }

    /// Advances the networking tick loop and dispatches global protocol
    /// messages such as time sync.
    ///
    /// A tick runs from one call to the next: this call, then
    /// [`Network::update_scene`], then the synchronized component updates.
    /// Every handler has had its pass on a message by the next call, so this
    /// call first drops whatever is still queued, and any component sync the
    /// host did not relay, before polling new messages.
    pub fn update(&mut self, time: &mut Time) {
        let expired = self.queue.expire();
        if expired > 0 {
            log::trace!("Expired {expired} network messages no handler consumed");
        }
        if let Some(server) = &mut self.server {
            server.discard_component_syncs();
        }
        self.accumulated_time += time.static_duration().as_secs_f32();
        self.tick_sync_timer += time.static_delta_time();
        while self.accumulated_time >= self.tick_period {
            self.accumulated_time -= self.tick_period;
            let duration = Duration::from_secs_f32(self.tick_period);
            if let Some(server) = &mut self.server {
                server.update(&mut self.queue, duration);
                if self.tick_sync_timer >= 1.0 {
                    if let Err(e) = server.broadcast_message_except(
                        0,
                        &GameMessage::SyncTime {
                            current_time: time.time(),
                        },
                    ) {
                        log::warn!("Failed to broadcast time sync: {e}");
                    }
                    self.tick_sync_timer = 0.0;
                }
            }
            self.client.update(&mut self.queue, duration);
        }
        self.queue
            .receive_messages(None, &mut (time, self.tick_rate), &mut NetworkTimeSync);
    }

    /// Applies queued scene-related replication messages.
    ///
    /// This consumes ownership, connection, prefab-spawn and destroy messages
    /// against `scene`, then relays the clients' component syncs on the host.
    /// Call this after [`Network::update`] in the main simulation loop.
    pub fn update_scene<'a>(
        &'a mut self,
        mut scene: &'a mut Scene,
        assets: &'a ReadOnlyAssetContext,
    ) {
        // One pass over the queue, so lifecycle messages for the same object
        // (spawn, ownership transfer, destroy) apply in the order they were
        // sent.
        self.queue.receive_messages(
            None,
            &mut (
                &mut self.client,
                &mut self.server,
                &mut scene,
                assets,
                &mut self.local_id,
            ),
            &mut NetworkReplicationSync,
        );
        // After the pass, so a spawn received with its first syncs is known.
        if let Some(server) = &mut self.server {
            server.relay_component_syncs(|network_object_id| {
                find_network_object(scene, network_object_id).map(|(_, owner_id)| owner_id)
            });
        }
    }

    /// Destroys `game_object` locally and tells the other peers to destroy
    /// their copy.
    ///
    /// Only the owner of a network object may destroy it. Returns `false`,
    /// leaving the object in place, when the local peer has no id yet, the
    /// object has no [`ComponentNetworkObject`], or another peer owns it.
    pub fn destroy_game_object(&mut self, scene: &mut Scene, game_object: GameObject) -> bool {
        let Some(client_id) = self.local_id else {
            return false;
        };
        let Some((network_object_id, owner_id)) = scene
            .read_component::<ComponentNetworkObject, _, _>(game_object, |c| (c.id, c.owner_id))
        else {
            return false;
        };
        if owner_id != client_id {
            log::warn!(
                "Refusing to destroy network object {network_object_id} owned by {owner_id}"
            );
            return false;
        }
        scene.delete(game_object);
        let message = GameMessage::DestroyGameObject {
            network_object_id,
            from_client_id: client_id,
        };
        let result = match &mut self.server {
            Some(server) => server.broadcast_message(&message),
            None => self.client.send_message(&message),
        };
        if let Err(e) = result {
            log::warn!("Failed to send DestroyGameObject: {e}");
        }
        true
    }

    /// Enables or disables `game_object`'s component of type `component_uuid`
    /// locally and tells the other peers to do the same with their copy,
    /// through the server.
    ///
    /// Only the owner of a network object may change it. Returns `false`,
    /// changing nothing, when the local peer has no id yet, the object has no
    /// [`ComponentNetworkObject`], another peer owns it, or the scene refuses
    /// the change; see [`Scene::set_component_enabled_dyn`].
    pub fn set_component_enabled(
        &mut self,
        scene: &mut Scene,
        game_object: GameObject,
        component_uuid: uuid::Uuid,
        enabled: bool,
    ) -> bool {
        let Some(client_id) = self.local_id else {
            return false;
        };
        let Some((network_object_id, owner_id)) = scene
            .read_component::<ComponentNetworkObject, _, _>(game_object, |c| (c.id, c.owner_id))
        else {
            return false;
        };
        if owner_id != client_id {
            log::warn!(
                "Refusing to change component {component_uuid} of network object                  {network_object_id} owned by {owner_id}"
            );
            return false;
        }
        if !scene.set_component_enabled_dyn(game_object, component_uuid, enabled) {
            return false;
        }
        let message = GameMessage::SetComponentEnabled {
            network_object_id,
            from_client_id: client_id,
            component_uuid,
            enabled,
        };
        let result = match &mut self.server {
            Some(server) => server.broadcast_message(&message),
            None => self.client.send_message(&message),
        };
        if let Err(e) = result {
            log::warn!("Failed to send SetComponentEnabled: {e}");
        }
        true
    }

    /// Returns whether this network resource is hosting a local server.
    pub fn is_host(&self) -> bool {
        self.server.is_some()
    }

    /// Returns the fixed tick period in seconds.
    pub fn tick_period(&self) -> TimeType {
        self.tick_period
    }

    /// Returns the fixed tick rate in hertz.
    pub fn tick_rate(&self) -> TimeType {
        self.tick_rate
    }

    /// Instantiates `prefab_ref` locally, assigns network object IDs from the
    /// local peer's range, and broadcasts the spawn to other peers.
    ///
    /// Returns `None` before the local peer has an id and a range, or when
    /// the range has too few ids left.
    pub fn instantiate_prefab(
        &mut self,
        scene: &mut Scene,
        prefab_ref: Ref<Prefab>,
    ) -> Option<GameObject> {
        let client_id = self.local_id?;
        self.client.network_ids.range()?;
        let prefab = prefab_ref.read();
        let prefab_root = match scene.instantiate_prefab(&prefab, None) {
            Ok(prefab_root) => prefab_root,
            Err(err) => {
                log::warn!("Failed to instantiate prefab {}: {err}", prefab_ref.id());
                return None;
            }
        };
        let objects = prefab_network_objects(scene, prefab_root);
        let mut network_object_ids = HashMap::with_capacity(objects.len());
        for &(index, _) in &objects {
            let Some(id) = self.client.network_ids.allocate() else {
                log::warn!(
                    "No network object ids left to spawn prefab {}",
                    prefab_ref.id()
                );
                discard_instance(scene, prefab_root, &objects);
                return None;
            };
            network_object_ids.insert(index, id);
        }
        assign_network_ids(scene, &objects, client_id, &network_object_ids);
        let message = GameMessage::SpawnPrefab {
            network_object_ids,
            from_client_id: client_id,
            prefab_id: prefab_ref.id(),
        };
        // A listen-server host has no connection of its own to send through.
        let result = match &mut self.server {
            Some(server) => server.broadcast_message(&message),
            None => self.client.send_message(&message),
        };
        if let Err(e) = result {
            log::warn!("Failed to send SpawnPrefab: {e}");
        }
        Some(prefab_root)
    }
}

struct NetworkTimeSync;
impl MessageHandler<(&mut Time, TimeType), GameMessage> for NetworkTimeSync {
    fn handle_message(
        &mut self,
        (time, tick_rate): &mut (&mut Time, TimeType),
        _sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SyncTime { current_time } => {
                time.set_time(*current_time + (1.0 / AbsoluteTime::from(*tick_rate)));
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

/// Message handler that applies connection lifecycle and ownership-transfer
/// messages to scene and client state.
///
/// Most callers should use [`Network::update_scene`] instead of invoking this
/// handler directly.
pub struct NetworkSceneSync;
type NetworkSceneSyncContext<'a, 'b> = (
    &'a mut Client,
    &'a mut Option<Server>,
    &'a mut &'b mut Scene,
    &'a mut Option<ClientId>,
);

impl MessageHandler<NetworkSceneSyncContext<'_, '_>, GameMessage> for NetworkSceneSync {
    fn handle_message(
        &mut self,
        (client, server, scene, local_id): &mut NetworkSceneSyncContext,
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::ServerEvent(ServerEvent::ClientConnected { client_id }) => {
                let Some(server) = server else {
                    return MessageHandlerResult::Consume;
                };
                let Some(id_range) = server.reserve_id_range(*client_id) else {
                    log::error!("No network object id range left for client {client_id}");
                    return MessageHandlerResult::Consume;
                };
                if let Err(e) = server.send_message(
                    *client_id,
                    &GameMessage::SelfConnected {
                        client_ids: server.client_ids(),
                        id_range,
                    },
                ) {
                    log::warn!("Failed to send SelfConnected to {client_id}: {e}");
                }
                if let Err(e) = server.broadcast_message_except(
                    *client_id,
                    &GameMessage::ClientConnected {
                        client_id: *client_id,
                    },
                ) {
                    log::warn!("Failed to broadcast ClientConnected: {e}");
                }
                MessageHandlerResult::Consume
            }
            GameMessage::ServerEvent(ServerEvent::ClientDisconnected { client_id }) => {
                let Some(server) = server else {
                    return MessageHandlerResult::Consume;
                };
                if let Err(e) = server.broadcast_message_except(
                    *client_id,
                    &GameMessage::ClientDisconnected {
                        client_id: *client_id,
                    },
                ) {
                    log::warn!("Failed to broadcast ClientDisconnected: {e}");
                }
                MessageHandlerResult::Consume
            }
            GameMessage::SelfConnected {
                client_ids,
                id_range,
            } => {
                #[allow(unused)]
                'client_logic: {
                    let self_client_id = client.client_id();
                    **local_id = self_client_id;
                    client.network_ids.assign(*id_range);
                    client.client_ids = client_ids.clone();
                    client.client_ids.retain(|cid| self_client_id != Some(*cid));
                    log::trace!("Self connected, local_id: {self_client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientConnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.push(*client_id);
                    log::trace!("Client Connected: {client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::ClientDisconnected { client_id } => {
                #[allow(unused)]
                'client_logic: {
                    client.client_ids.retain(|&id| id != *client_id);
                    log::trace!("Client Disconnected: {client_id:?}");
                };
                MessageHandlerResult::Consume
            }
            GameMessage::TransferOwnership {
                network_object_id,
                from_client_id,
                to_client_id,
            } => {
                let acting_client_id = sender.acting_client(*from_client_id);
                let Some(game_object) = find_network_object(scene, *network_object_id)
                    .filter(|(_, owner_id)| *owner_id == acting_client_id)
                    .map(|(game_object, _)| game_object)
                else {
                    return MessageHandlerResult::Consume;
                };
                'server_logic: {
                    let Some(server) = server else {
                        break 'server_logic;
                    };
                    log::trace!("SERVER - Transferring ownership");
                    if let Err(e) = server.broadcast_message(message) {
                        log::warn!("Failed to broadcast TransferOwnership: {e}");
                    }
                }
                let transferred =
                    scene.write_component::<ComponentNetworkObject, _>(game_object, |c_netobj| {
                        c_netobj.owner_id = *to_client_id;
                    });
                if transferred.is_some() {
                    log::trace!("CLIENT - Transferring ownership");
                }
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

/// Applies a `DestroyGameObject` request delivered by `sender`.
///
/// A request from a client that does not own the object is dropped. On the
/// server, an accepted request is relayed to every other client; the server
/// may not hold a copy of every object, so it relays requests for objects it
/// does not know and each receiving peer checks ownership itself.
pub(crate) fn handle_destroy(
    server: &mut Option<Server>,
    scene: &mut Scene,
    sender: MessageSender,
    network_object_id: NetworkObjectId,
    from_client_id: ClientId,
    message: &GameMessage,
) {
    let acting_client_id = sender.acting_client(from_client_id);
    let target = find_network_object(scene, network_object_id);
    if let Some((_, owner_id)) = target {
        if owner_id != acting_client_id {
            log::warn!(
                "Rejected destroy of network object {network_object_id} owned by {owner_id} \
                 from {acting_client_id}"
            );
            return;
        }
    }
    if let Some(server) = server {
        if let Err(e) = server.broadcast_message_except(acting_client_id, message) {
            log::warn!("Failed to broadcast DestroyGameObject: {e}");
        }
    }
    if let Some((game_object, _)) = target {
        scene.delete(game_object);
    }
}

/// Applies a `SetComponentEnabled` request delivered by `sender`, as
/// [`handle_destroy`] applies a destroy: a request from a client that does
/// not own the object is dropped, and the server relays an accepted one to
/// every other client, including for an object it does not hold.
pub(crate) fn handle_component_enabled(
    server: &mut Option<Server>,
    scene: &mut Scene,
    sender: MessageSender,
    message: &GameMessage,
) {
    let GameMessage::SetComponentEnabled {
        network_object_id,
        from_client_id,
        component_uuid,
        enabled,
    } = message
    else {
        return;
    };
    let acting_client_id = sender.acting_client(*from_client_id);
    let target = find_network_object(scene, *network_object_id);
    if let Some((_, owner_id)) = target {
        if owner_id != acting_client_id {
            log::warn!(
                "Rejected change of component {component_uuid} on network object                  {network_object_id} owned by {owner_id} from {acting_client_id}"
            );
            return;
        }
    }
    if let Some(server) = server {
        if let Err(e) = server.broadcast_message_except(acting_client_id, message) {
            log::warn!("Failed to broadcast SetComponentEnabled: {e}");
        }
    }
    if let Some((game_object, _)) = target {
        scene.set_component_enabled_dyn(game_object, *component_uuid, *enabled);
    }
}

/// Finds the game object carrying network object `network_object_id` and
/// returns it with its owner.
fn find_network_object(
    scene: &mut Scene,
    network_object_id: NetworkObjectId,
) -> Option<(GameObject, ClientId)> {
    let game_object = scene.find_network_object(network_object_id)?;
    scene
        .read_component::<ComponentNetworkObject, _, _>(game_object, |c| c.owner_id)
        .map(|owner_id| (game_object, owner_id))
}

/// Message handler that offers each message to [`NetworkSceneSync`], then to
/// [`NetworkPrefabSync`], so every scene replication message is handled in
/// one FIFO pass over the queue.
struct NetworkReplicationSync;
impl MessageHandler<NetworkPrefabSyncContext<'_, '_>, GameMessage> for NetworkReplicationSync {
    fn handle_message(
        &mut self,
        context: &mut NetworkPrefabSyncContext,
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        let (client, server, scene, assets, local_id) = context;
        let result = NetworkSceneSync.handle_message(
            &mut (&mut **client, &mut **server, &mut **scene, &mut **local_id),
            sender,
            message,
        );
        match result {
            MessageHandlerResult::Consume => MessageHandlerResult::Consume,
            MessageHandlerResult::Ignore => NetworkPrefabSync.handle_message(
                &mut (client, server, scene, assets, local_id),
                sender,
                message,
            ),
        }
    }
}

struct NetworkPrefabSync;
type NetworkPrefabSyncContext<'a, 'b> = (
    &'a mut Client,
    &'a mut Option<Server>,
    &'a mut &'b mut Scene,
    &'a ReadOnlyAssetContext,
    &'a mut Option<ClientId>,
);
impl MessageHandler<NetworkPrefabSyncContext<'_, '_>, GameMessage> for NetworkPrefabSync {
    fn handle_message(
        &mut self,
        (_client, server, scene, context, local_id): &mut NetworkPrefabSyncContext,
        sender: MessageSender,
        message: &GameMessage,
    ) -> MessageHandlerResult {
        match message {
            GameMessage::SpawnPrefab {
                from_client_id,
                network_object_ids,
                prefab_id,
            } => {
                let acting_client_id = sender.acting_client(*from_client_id);
                // The server checks the ids before it instantiates or relays
                // anything, so a refused spawn reaches no peer.
                if let Some(server) = server {
                    if let Err(reason) =
                        check_spawn_ids(server, scene, acting_client_id, network_object_ids)
                    {
                        log::warn!(
                            "Refused spawn of prefab {prefab_id} from {acting_client_id}: {reason}"
                        );
                        return MessageHandlerResult::Consume;
                    }
                }
                let instantiate = (**local_id).is_some_and(|id| id != acting_client_id);
                if instantiate {
                    if let Err(reason) = instantiate_spawn(
                        scene,
                        context,
                        acting_client_id,
                        *prefab_id,
                        network_object_ids,
                    ) {
                        log::warn!(
                            "Refused spawn of prefab {prefab_id} from {acting_client_id}: {reason}"
                        );
                        return MessageHandlerResult::Consume;
                    }
                }
                // The server relays the spawn to every other client once it
                // has instantiated it like any other peer.
                if let Some(server) = server {
                    if let Err(e) = server.broadcast_message_except(acting_client_id, message) {
                        log::warn!("Failed to broadcast SpawnPrefab: {e}");
                    }
                }
                MessageHandlerResult::Consume
            }
            // Handled in the same pass as `SpawnPrefab`, so a destroy that
            // follows a spawn is applied and relayed after it.
            GameMessage::DestroyGameObject {
                network_object_id,
                from_client_id,
            } => {
                handle_destroy(
                    server,
                    scene,
                    sender,
                    *network_object_id,
                    *from_client_id,
                    message,
                );
                MessageHandlerResult::Consume
            }
            // In the same pass too, so a change that follows a spawn applies
            // to the spawned object.
            GameMessage::SetComponentEnabled { .. } => {
                handle_component_enabled(server, scene, sender, message);
                MessageHandlerResult::Consume
            }
            _ => MessageHandlerResult::Ignore,
        }
    }
}

/// Returns, in prefab order, the index of each object of the prefab instance
/// rooted at `root` that carries a [`ComponentNetworkObject`], with the
/// object. The index counts every object of the instance, depth first, so it
/// names the same object on every peer.
fn prefab_network_objects(scene: &Scene, root: GameObject) -> Vec<(u32, GameObject)> {
    fn visit(
        scene: &Scene,
        game_object: GameObject,
        index: &mut u32,
        objects: &mut Vec<(u32, GameObject)>,
    ) {
        if scene
            .read_component::<ComponentNetworkObject, _, _>(game_object, |_| ())
            .is_some()
        {
            objects.push((*index, game_object));
        }
        *index += 1;
        for child in scene.children_ordered(game_object).collect::<Vec<_>>() {
            visit(scene, child, index, objects);
        }
    }
    let mut objects = Vec::new();
    visit(scene, root, &mut 0, &mut objects);
    objects
}

/// Deletes the prefab instance rooted at `root`, whose network objects are
/// `objects`.
///
/// Deletion waits for the next flush, so the instance first drops its network
/// objects: the index then forgets the prefab's ids, and no lookup resolves to
/// the discarded instance meanwhile.
fn discard_instance(scene: &mut Scene, root: GameObject, objects: &[(u32, GameObject)]) {
    for (_, game_object) in objects {
        scene.remove_component::<ComponentNetworkObject>(*game_object);
    }
    scene.delete(root);
}

/// Gives each of `objects` its id from `ids` and `owner_id` as its owner.
fn assign_network_ids(
    scene: &mut Scene,
    objects: &[(u32, GameObject)],
    owner_id: ClientId,
    ids: &HashMap<u32, NetworkObjectId>,
) {
    for (index, game_object) in objects {
        // Written through the scene so its network object index sees the id.
        scene.write_component::<ComponentNetworkObject, _>(*game_object, |c_netobj| {
            c_netobj.owner_id = owner_id;
            if let Some(&id) = ids.get(index) {
                c_netobj.id = id;
            }
        });
    }
}

/// Checks the ids of a spawn the server received from `client_id`.
///
/// Every id must come from the range the server reserved for `client_id`,
/// appear once in the spawn and belong to no object in `scene`. Ids from
/// another range are reserved by another peer, or by the scene for range 0.
///
/// Existing ids are looked up in the scene's network object index, which the
/// scene updates on every network object add, write, removal and deletion,
/// without the scan [`Scene::find_network_object`] repairs a miss with: every
/// id of a valid spawn misses.
fn check_spawn_ids(
    server: &Server,
    scene: &Scene,
    client_id: ClientId,
    ids: &HashMap<u32, NetworkObjectId>,
) -> Result<(), String> {
    let Some(range) = server.id_range(client_id) else {
        return Err(String::from("its sender has no network object id range"));
    };
    let mut seen = HashSet::with_capacity(ids.len());
    let duplicated: BTreeSet<_> = ids.values().filter(|&&id| !seen.insert(id)).collect();
    if !duplicated.is_empty() {
        return Err(format!("it names ids {duplicated:?} more than once"));
    }
    let foreign: BTreeSet<_> = ids
        .values()
        .filter(|&&id| network_id_range(id) != range)
        .collect();
    if !foreign.is_empty() {
        return Err(format!(
            "ids {foreign:?} are outside its range {range}, reserved by another peer"
        ));
    }
    let existing: BTreeSet<_> = ids
        .values()
        .filter(|&&id| scene.indexed_network_object(id).is_some())
        .collect();
    if !existing.is_empty() {
        return Err(format!("ids {existing:?} already exist in the scene"));
    }
    Ok(())
}

/// Instantiates a spawn of `prefab_id` received from `owner_id` and gives
/// its network objects the spawn's ids.
///
/// A spawn whose ids do not name exactly the prefab's network objects is
/// undone and refused, on the server and on every client: an object left
/// without an id would keep its prefab's, which other spawns share. A prefab
/// this peer cannot load is skipped; the server then relays the spawn after
/// checking only its ids, and each client that can load the prefab checks
/// the rest.
fn instantiate_spawn(
    scene: &mut Scene,
    context: &ReadOnlyAssetContext,
    owner_id: ClientId,
    prefab_id: uuid::Uuid,
    ids: &HashMap<u32, NetworkObjectId>,
) -> Result<(), String> {
    let Some(prefab_ref) = context
        .registries
        .assets
        .read()
        .load_by_id::<Prefab>(prefab_id)
        .ok()
    else {
        return Ok(());
    };
    let prefab = prefab_ref.read();
    let prefab_root = match scene.instantiate_prefab(&prefab, None) {
        Ok(prefab_root) => prefab_root,
        Err(err) => {
            log::warn!("Failed to instantiate spawned prefab {prefab_id}: {err}");
            return Ok(());
        }
    };
    let objects = prefab_network_objects(scene, prefab_root);
    let expected: BTreeSet<_> = objects.iter().map(|(index, _)| *index).collect();
    let named: BTreeMap<_, _> = ids.iter().collect();
    if !named.keys().copied().copied().eq(expected.iter().copied()) {
        discard_instance(scene, prefab_root, &objects);
        return Err(format!(
            "its ids {named:?} do not name the prefab's network objects {expected:?}"
        ));
    }
    // The spawning peer owns the replicated objects on every peer.
    assign_network_ids(scene, &objects, owner_id, ids);
    Ok(())
}
