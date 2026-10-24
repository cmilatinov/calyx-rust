use bimap::BiHashMap;
use legion::storage::ComponentTypeId;
use legion::world::{Allocate, Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use nalgebra_glm::Mat4;
use serde::de::DeserializeSeed;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::component::{Component, ComponentEventContext, ComponentTransform};
use crate::component::{ComponentCamera, ComponentID};
use crate::context::{ReadOnlyAssetContext, ReadOnlyRegistryContext};
use crate::input::Input;
use crate::math::Transform;
use crate::net::{ComponentNetworkObject, Network, NetworkObjectId};
use crate::physics::PhysicsContext;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::resource::ResourceMap;
use crate::scene::change_set::ChangeSet;
use crate::scene::game_object_store::GameObjectStore;
use crate::scene::network_index::NetworkObjectIndex;
use crate::scene::scene_graph::SceneGraph;
use crate::scene::transform_cache::TransformCache;
use crate::scene::{Prefab, SceneError, SceneView};
use crate::utils::{ContextSeed, TypeUuid};

use super::scene_graph::{self, SiblingDir};

/// Lightweight handle to a game object stored inside a [`Scene`].
///
/// The handle contains both the Legion entity and the scene-graph node index so
/// hierarchy and ECS lookups can stay cheap. Persist this handle only for the
/// lifetime of the scene instance; for serialized references use
/// [`GameObjectRef`](crate::scene::GameObjectRef).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameObject {
    /// Internal scene-graph node for hierarchy operations.
    pub node: petgraph::stable_graph::NodeIndex,
    /// Legion entity that stores the object's components.
    pub entity: Entity,
}

/// Serializable representation of a scene or subtree.
///
/// Every map is ordered by UUID, so saving the same scene always writes the
/// same file and an edit changes only its own lines. Component payloads are
/// `serde_json` values, whose objects keep their keys sorted too.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    /// Component payloads keyed by game object UUID and then component type
    /// UUID.
    pub components: BTreeMap<Uuid, BTreeMap<Uuid, serde_json::Value>>,
    /// Each parent's children in sibling order, keyed by parent UUID.
    pub hierarchy: BTreeMap<Uuid, Vec<Uuid>>,
    /// Top-level objects in sibling order: the root's children for a whole
    /// scene, or the subtree's own root for a subtree.
    pub roots: Vec<Uuid>,
}

/// Detached snapshot of a scene that can be restored later against a registry
/// context.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub struct SceneSnapshot {
    data: SceneData,
}

/// A scene's Legion world, readable throughout the engine through `Deref`.
///
/// The world itself is private to this module, so mutable component storage is
/// only reached through [`Scene`]'s write methods, which observe every write.
pub(crate) struct SceneWorld(World);

impl std::ops::Deref for SceneWorld {
    type Target = World;

    fn deref(&self) -> &World {
        &self.0
    }
}

/// What the editor closure of [`Scene::edit_component_dyn`] returns: its own
/// value and whether it wrote the component.
pub struct ComponentEdit<R> {
    /// The editor closure's return value.
    pub value: R,
    /// Whether the editor wrote the component. The scene reports a write
    /// only when this is true, so an editor that changes the component must
    /// set it.
    pub written: bool,
}

/// Runtime scene containing ECS state, hierarchy state, and physics state for a
/// collection of game objects.
///
/// Every component write goes through a scene method, so the scene sees each
/// one: [`Scene::world`] only hands out shared access, and the methods that
/// add, write or remove components all report the write to the scene's one
/// observation point, which keeps cached world transforms current.
///
/// Objects enter the scene only through its spawn operation, which registers
/// an object's entity and id together, and an object's id is read-only: the
/// observation point puts it back after any write to the object's
/// [`ComponentID`]. Lookups by entity and by id therefore always agree.
#[derive(TypeUuid)]
#[uuid = "9946a2e7-e022-447e-8e60-528da548087f"]
pub struct Scene {
    /// Legion world that stores all scene components. The engine reads it
    /// directly; only this module reaches its mutable side.
    pub(crate) world: SceneWorld,
    /// Physics state coupled to the scene.
    pub physics: PhysicsContext,
    pub(crate) graph: SceneGraph,
    pub(crate) store: GameObjectStore,
    pub(crate) transforms: TransformCache,
    /// Entities written per component type since each type's consumer last
    /// took them; see [`Scene::component_written`].
    pub(crate) changes: ChangeSet,
    /// Index of network objects by id, listing every claimant of an id.
    /// Entries are hints that lookups check and repair; see
    /// [`Scene::find_network_object`].
    network_objects: NetworkObjectIndex,
    root: GameObject,
    camera: Option<GameObject>,
    registries: ReadOnlyRegistryContext,
    started: bool,
    /// Hands out the entities of the objects this scene spawns: the entity
    /// counter of the engine build that created the scene; see
    /// [`allocate_entity`].
    pub(crate) allocate_entity: fn() -> Entity,
}

/// Hands out an entity from this engine build's copy of Legion's process-wide
/// entity counter.
///
/// A project assembly links its own copy of the engine and of Legion, and so
/// its own copy of that counter, which starts over and hands out the entities
/// the editor's copy already did. [`Scene::new`] keeps a pointer to the copy
/// of this function in the build that creates the scene, so the objects any
/// code spawns into the scene, the assembly's included, take their entities
/// from that one counter.
fn allocate_entity() -> Entity {
    Allocate::new()
        .next()
        .expect("Legion's entity allocator never runs out")
}

impl Scene {
    /// Creates an empty scene with a generated root object.
    pub fn new(assets: ReadOnlyRegistryContext) -> Self {
        let mut world: World = Default::default();
        let mut graph = SceneGraph::default();
        let entity = allocate_entity();
        world.push_with_id(entity, ());
        let root = graph.attach(entity, None);
        let id = Uuid::new_v4();
        world.entry(entity).unwrap().add_component(ComponentID {
            id,
            name: String::from("Root"),
            visible: true,
        });
        let mut store = GameObjectStore::default();
        store.insert(id, root);
        Self {
            world: SceneWorld(world),
            physics: Default::default(),
            graph,
            store,
            transforms: TransformCache::default(),
            changes: ChangeSet::default(),
            network_objects: NetworkObjectIndex::default(),
            root,
            camera: Default::default(),
            registries: assets,
            started: false,
            allocate_entity,
        }
    }
}

impl Asset for Scene {
    fn asset_name() -> &'static str
    where
        Self: Sized,
    {
        "Scene"
    }

    fn file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxscene"]
    }

    fn from_file(
        assets: &ReadOnlyAssetContext,
        path: &Path,
    ) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        LoadedAsset::<Self>::from_json_file_ctx(assets, path)
    }
}

impl Serialize for Scene {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let data: SceneData = self.into();
        data.serialize(serializer)
    }
}

impl<'de> DeserializeSeed<'de> for ContextSeed<'de, ReadOnlyAssetContext, Scene> {
    type Value = Scene;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Scene::from((
            self.context,
            SceneData::deserialize(deserializer)?,
        )))
    }
}

impl Clone for Scene {
    fn clone(&self) -> Self {
        self.snapshot().into_scene(&self.registries)
    }
}

impl SceneSnapshot {
    /// Restores this snapshot into a new [`Scene`] using `registries` to
    /// deserialize components.
    pub fn into_scene(self, registries: &ReadOnlyRegistryContext) -> Scene {
        (registries, self.data).into()
    }
}

impl From<(&ReadOnlyRegistryContext, SceneData)> for Scene {
    fn from((registries, value): (&ReadOnlyRegistryContext, SceneData)) -> Self {
        let mut scene = registries.scene();
        let registry = registries.components.read();
        for (game_object_id, mut components) in value.components {
            // The object's key is its identity, since the hierarchy refers to
            // objects by key; the stored id component only supplies the rest.
            let id = components
                .remove(&ComponentID::type_uuid())
                .and_then(|data| match serde_json::from_value::<ComponentID>(data) {
                    Ok(id) => Some(id),
                    Err(err) => {
                        log::warn!("Object {game_object_id} has an invalid id component: {err}");
                        None
                    }
                })
                .unwrap_or_default();
            let game_object = scene.spawn(
                ComponentID {
                    id: game_object_id,
                    ..id
                },
                None,
            );
            for (component_id, data) in components {
                let Some(component) = registry.component(component_id) else {
                    continue;
                };
                let instance = match component.deserialize(&data) {
                    Ok(instance) => instance,
                    Err(err) => {
                        log::warn!(
                            "Skipping component {} that failed to deserialize: {err}",
                            component.type_name()
                        );
                        continue;
                    }
                };
                scene.bind_instance(game_object, component, instance);
            }
        }
        for (parent_id, children) in value.hierarchy {
            let Some(parent) = scene.find(parent_id) else {
                continue;
            };
            for child_id in children {
                let Some(child) = scene.find(child_id) else {
                    continue;
                };
                if let Err(err) = scene.set_parent(child, Some(parent)) {
                    log::warn!("Skipping invalid scene hierarchy entry: {err}");
                }
            }
        }
        // Each object was spawned under the root, so moving the top-level
        // ones to the end of the root's children in turn puts them in their
        // stored order.
        for root_id in value.roots {
            let Some(game_object) = scene.find(root_id) else {
                continue;
            };
            if scene.parent(game_object) != Some(scene.root) {
                continue;
            }
            if let Err(err) = scene.set_parent(game_object, None) {
                log::warn!("Skipping invalid scene root entry: {err}");
            }
        }
        scene
    }
}

impl From<(&ReadOnlyAssetContext, SceneData)> for Scene {
    fn from((assets, value): (&ReadOnlyAssetContext, SceneData)) -> Self {
        (&assets.registries, value).into()
    }
}

impl From<&Scene> for SceneData {
    fn from(scene: &Scene) -> Self {
        let mut data = SceneData::default();
        // Walking the graph in sibling order records each parent's children
        // in that order. The root belongs to the scene instance, not to its
        // content: a loaded scene creates its own.
        for game_object in scene.graph.descendants_ordered(scene.root).skip(1) {
            scene.serialize_game_object(game_object, scene.uuid(game_object), &mut data);
        }
        data.roots = scene
            .children_ordered(scene.root)
            .map(|game_object| scene.uuid(game_object))
            .collect();
        data
    }
}

impl From<(&Scene, GameObject)> for SceneData {
    fn from((scene, game_object): (&Scene, GameObject)) -> Self {
        let mut data = SceneData {
            roots: vec![scene.uuid(game_object)],
            ..Default::default()
        };
        let game_objects: Vec<GameObject> = scene.graph.descendants_ordered(game_object).collect();
        let subtree = game_objects.iter().copied().collect::<HashSet<_>>();
        game_objects.into_iter().for_each(|game_object| {
            let id = scene.uuid(game_object);
            scene.serialize_game_object_filtered(game_object, id, &mut data, Some(&subtree));
        });
        data
    }
}

impl Scene {
    /// Captures a serializable clone of the current scene state.
    pub fn snapshot(&self) -> SceneSnapshot {
        SceneSnapshot { data: self.into() }
    }

    pub(crate) fn registries(&self) -> &ReadOnlyRegistryContext {
        &self.registries
    }

    /// Restores `snapshot` into a new scene using this scene's registry
    /// context.
    pub fn restore_snapshot(&self, snapshot: SceneSnapshot) -> Self {
        snapshot.into_scene(&self.registries)
    }

    pub(crate) fn game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.store.game_object_from_entity(entity)
    }

    pub(crate) fn serialize_game_object(
        &self,
        game_object: GameObject,
        game_object_id: Uuid,
        data: &mut SceneData,
    ) {
        self.serialize_game_object_filtered(game_object, game_object_id, data, None);
    }

    fn serialize_game_object_filtered(
        &self,
        game_object: GameObject,
        game_object_id: Uuid,
        data: &mut SceneData,
        parent_filter: Option<&HashSet<GameObject>>,
    ) {
        'insert_hierarchy: {
            let Some(parent) = self.parent(game_object) else {
                break 'insert_hierarchy;
            };
            // Top-level objects record no parent: the root is never
            // serialized, unless a subtree explicitly includes it.
            let parent_included = match parent_filter {
                Some(filter) => filter.contains(&parent),
                None => parent != self.root,
            };
            if !parent_included {
                break 'insert_hierarchy;
            }
            data.hierarchy
                .entry(self.uuid(parent))
                .or_default()
                .push(game_object_id);
        }

        for (component_id, component) in self.registries.components.read().components() {
            let Some(entry) = self.entry(game_object) else {
                continue;
            };
            let Some(instance) = component.get_instance(&entry) else {
                continue;
            };
            let Some(value) = instance.serialize() else {
                continue;
            };
            data.components
                .entry(game_object_id)
                .or_default()
                .insert(*component_id, value);
        }
    }
}

#[allow(unused)]
impl Scene {
    /// Returns the synthetic root object that owns all top-level objects.
    pub fn root(&self) -> GameObject {
        self.root
    }

    /// Returns the UUID assigned to the synthetic root object.
    pub fn root_id(&self) -> Uuid {
        self.uuid(self.root)
    }

    /// Iterates direct children of the root in sibling order.
    pub fn root_objects(&self) -> impl Iterator<Item = GameObject> + '_ {
        self.children_ordered(self.root)
    }

    /// Returns the first root object, which is typically the authored prefab
    /// root for imported scenes.
    pub fn prefab_root(&self) -> Option<GameObject> {
        self.root_objects().next()
    }

    /// Creates a new game object under `parent`.
    ///
    /// When `id` is `None`, the scene generates a fresh [`ComponentID`] and a
    /// default [`ComponentTransform`].
    pub fn create(&mut self, id: Option<ComponentID>, parent: Option<GameObject>) -> GameObject {
        let is_default_id = id.is_none();
        let mut id = id.unwrap_or_default();
        if is_default_id {
            id.name = self.store.next_name();
        }
        let game_object = self.spawn(id, parent);
        self.add_component(game_object, ComponentTransform::default());
        game_object
    }

    /// Marks `game_object` and its descendants for deletion on the next
    /// [`Scene::flush_deletes`] or [`Scene::prepare`] call.
    pub fn delete(&mut self, game_object: GameObject) {
        self.store.mark_for_deletion(game_object);
    }

    /// Serializes `game_object` and its descendants into a prefab asset.
    pub fn create_prefab(&self, game_object: GameObject) -> Prefab {
        Prefab {
            data: (self, game_object).into(),
            root: self.uuid(game_object),
        }
    }

    /// Checks that `prefab` is a single well-formed tree rooted at its root
    /// object, so instantiation cannot fail midway or leave stray objects.
    fn validate_prefab(&self, prefab: &Prefab) -> Result<(), SceneError> {
        let invalid = |reason: String| Err(SceneError::InvalidPrefab(reason));
        let components = &prefab.data.components;
        if !components.contains_key(&prefab.root) {
            return invalid(format!("root object {} is missing", prefab.root));
        }
        let component_registry = self.registries.components.read();
        let id_component = component_registry.component(ComponentID::type_uuid());
        for (object_id, object_components) in components {
            let has_id = id_component.as_ref().is_some_and(|component| {
                object_components
                    .get(&ComponentID::type_uuid())
                    .is_some_and(|data| component.deserialize(data).is_ok())
            });
            if !has_id {
                return invalid(format!("object {object_id} has no valid ID component"));
            }
        }
        let mut parents = HashMap::new();
        for (parent_id, children) in prefab.data.hierarchy.iter() {
            if !components.contains_key(parent_id) {
                return invalid(format!("hierarchy parent {parent_id} is missing"));
            }
            for child_id in children {
                if !components.contains_key(child_id) {
                    return invalid(format!("hierarchy child {child_id} is missing"));
                }
                if *child_id == prefab.root {
                    return invalid(format!("root object {child_id} has a parent"));
                }
                if parents.insert(*child_id, *parent_id).is_some() {
                    return invalid(format!("object {child_id} has multiple parents"));
                }
            }
        }
        for object_id in components.keys() {
            let mut current = *object_id;
            let mut steps = 0;
            while current != prefab.root {
                let Some(parent_id) = parents.get(&current) else {
                    return invalid(format!("object {object_id} is not reachable from the root"));
                };
                steps += 1;
                if steps > components.len() {
                    return Err(SceneError::HierarchyCycle);
                }
                current = *parent_id;
            }
        }
        Ok(())
    }
    /// Instantiates `prefab` into the scene and remaps any embedded
    /// [`GameObjectRef`](crate::scene::GameObjectRef) values to the newly created objects. Remapping happens
    /// while each component is deserialized, so references inside nested
    /// structs, lists and enum variants are remapped too. A reference to an
    /// object outside the prefab is left unchanged with a warning.
    ///
    /// Returns [`SceneError::InvalidPrefab`] when the prefab data is malformed.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &Prefab,
        parent: Option<GameObject>,
    ) -> Result<GameObject, SceneError> {
        self.validate_prefab(prefab)?;
        let mut id_mapping = prefab
            .data
            .components
            .keys()
            .map(|game_object_id| (*game_object_id, Uuid::new_v4()))
            .collect::<BiHashMap<_, _>>();

        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let type_registry_ref = self.registries.types.clone();
        let type_registry = type_registry_ref.read();
        for (game_object_id, components) in prefab.data.components.iter() {
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).ok_or_else(|| {
                SceneError::InvalidPrefab(format!("object {game_object_id} has no mapped id"))
            })?;
            let id = components
                .get(&ComponentID::type_uuid())
                .and_then(|data| serde_json::from_value::<ComponentID>(data.clone()).ok())
                .ok_or_else(|| {
                    SceneError::InvalidPrefab(format!("object {game_object_id} has no valid id"))
                })?;
            self.spawn(
                ComponentID {
                    id: new_game_object_id,
                    ..id
                },
                None,
            );
        }

        // Every `GameObjectRef` in the components is remapped to the
        // instance's objects, wherever serde reaches it.
        for (game_object_id, components) in prefab.data.components.iter() {
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).ok_or_else(|| {
                SceneError::InvalidPrefab(format!("object {game_object_id} has no mapped id"))
            })?;
            let game_object = self.find(new_game_object_id).ok_or_else(|| {
                SceneError::InvalidPrefab(format!("object {game_object_id} was not created"))
            })?;
            for (component_id, data) in components {
                // The object was spawned with its id component.
                if *component_id == ComponentID::type_uuid() {
                    continue;
                }
                let Some(TypeInfo::Struct(_)) = type_registry.type_info_by_id(*component_id) else {
                    continue;
                };
                let Some(component) = component_registry.component(*component_id) else {
                    continue;
                };
                let instance = match component.deserialize_remapped(data, &id_mapping) {
                    Ok(instance) => instance,
                    Err(err) => {
                        log::warn!(
                            "Skipping prefab component {} that failed to deserialize: {err}",
                            component.type_name()
                        );
                        continue;
                    }
                };
                self.bind_instance(game_object, component, instance);
            }
        }

        for (parent_id, children) in prefab.data.hierarchy.iter() {
            let Some(new_parent_id) = id_mapping.get_by_left(parent_id) else {
                continue;
            };
            let Some(parent) = self.find(*new_parent_id) else {
                continue;
            };
            for child_id in children {
                let Some(new_child_id) = id_mapping.get_by_left(child_id) else {
                    continue;
                };
                let Some(child) = self.find(*new_child_id) else {
                    continue;
                };
                self.set_parent(child, Some(parent))?;
            }
        }

        let prefab_uuid = id_mapping.get_by_left(&prefab.root).ok_or_else(|| {
            SceneError::InvalidPrefab(format!("root object {} is missing", prefab.root))
        })?;
        let game_object = self.find(*prefab_uuid).ok_or_else(|| {
            SceneError::InvalidPrefab(format!("root object {} was not created", prefab.root))
        })?;

        if let Some(parent) = parent {
            self.set_parent(game_object, Some(parent))?;
        }

        Ok(game_object)
    }

    /// Reparents `game_object` under `parent`, appending it after existing
    /// siblings. Fails with [`SceneError::HierarchyCycle`] when `parent` is
    /// inside `game_object`'s own subtree.
    pub fn set_parent(
        &mut self,
        game_object: GameObject,
        parent: Option<GameObject>,
    ) -> Result<(), SceneError> {
        self.set_parent_with_sibling(game_object, parent, None)
    }

    /// Reparents `game_object` and optionally inserts it relative to `sibling`.
    pub fn set_parent_with_sibling(
        &mut self,
        game_object: GameObject,
        parent: Option<GameObject>,
        sibling: Option<(GameObject, SiblingDir)>,
    ) -> Result<(), SceneError> {
        let parent = parent.unwrap_or(self.root);
        self.graph.set_parent(game_object, parent, sibling)?;
        self.transforms.mark_dirty_subtree(game_object, &self.graph);
        Ok(())
    }

    /// Returns the insertion index that would place `sibling` before or after
    /// `dir` under `parent`.
    pub fn index_in_parent(
        &self,
        parent: GameObject,
        sibling: GameObject,
        dir: SiblingDir,
    ) -> Option<i32> {
        self.graph.index_in_parent(parent, sibling, dir)
    }

    /// Returns the active camera object for rendering.
    ///
    /// When an explicit camera override has not been selected, the first enabled
    /// [`ComponentCamera`] in the scene is returned.
    pub fn main_camera(&self) -> Option<(GameObject, &ComponentCamera)> {
        let mut query = <(Entity, &ComponentTransform, &ComponentCamera)>::query();
        query
            .iter(&self.world.0)
            .filter_map(|(e, _t, c)| self.game_object_from_entity(*e).map(|go| (go, c)))
            .find(|(go, c)| {
                if let Some(camera) = &self.camera {
                    go == camera
                } else {
                    c.enabled
                }
            })
    }

    /// Spawns a game object under `parent`, or under the root when `parent`
    /// is `None`, carrying the identity `id` and no other component.
    ///
    /// This is the one way an object enters the scene: it registers the
    /// object's entity and id together, so lookups by either agree from the
    /// start. When `id.id` is nil or already held by another object, the
    /// object gets a fresh id instead, so no two objects share one.
    pub(crate) fn spawn(&mut self, mut id: ComponentID, parent: Option<GameObject>) -> GameObject {
        if id.id.is_nil() || self.store.contains(id.id) {
            let fresh = Uuid::new_v4();
            log::warn!(
                "Game object id {} is nil or taken; spawning with {fresh}",
                id.id
            );
            id.id = fresh;
        }
        let entity = self.new_entity();
        self.world.0.push_with_id(entity, ());
        let game_object = self.graph.attach(entity, Some(parent.unwrap_or(self.root)));
        self.transforms.remove(game_object.node);
        self.store.insert(id.id, game_object);
        self.add_component(game_object, id);
        game_object
    }

    /// Takes an entity no object in the scene holds from the scene's
    /// allocator. Legion's `push_with_id` replaces whatever entity holds the
    /// id it is given, so an entity already in the world, which only an
    /// allocator other than the one that numbered the world hands out, is
    /// skipped rather than taken over.
    fn new_entity(&self) -> Entity {
        loop {
            let entity = (self.allocate_entity)();
            if !self.world.contains(entity) {
                return entity;
            }
            log::error!("Skipping entity {entity:?}, which an object in the scene already holds");
        }
    }

    /// Adds a concrete component instance to `game_object`, replacing any
    /// component of the same type.
    pub fn add_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) {
        self.write_entry(game_object, ComponentTypeId::of::<T>(), |entry| {
            entry.add_component(component);
            Some(())
        });
    }

    /// Adds the type-erased `instance` of `component`'s type to `game_object`.
    /// Returns `false` when the object is missing or `instance` is another
    /// type.
    fn bind_instance(
        &mut self,
        game_object: GameObject,
        component: &dyn Component,
        instance: Box<dyn crate::reflect::Reflect>,
    ) -> bool {
        self.write_entry(game_object, component.component_type_id(), |entry| {
            component.bind_instance(entry, instance).then_some(())
        })
        .is_some()
    }

    /// Creates and binds a component by reflected type UUID.
    ///
    /// If the component type has a registered reset hook, the hook is executed
    /// after the instance is bound.
    pub fn bind_component_dyn(&mut self, game_object: GameObject, type_uuid: Uuid) {
        let type_registry_ref = self.registries.types.clone();
        let type_registry = type_registry_ref.read();
        let Some(meta) = type_registry.trait_meta::<ReflectDefault>(type_uuid) else {
            return;
        };
        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let Some(component) = component_registry.component(type_uuid) else {
            return;
        };
        let default_instance = meta.default();
        if self.bind_instance(game_object, component, default_instance) {
            // Call the registered reset if one exists for this component type.
            if let Some(resetter) = component_registry.reset_component(type_uuid) {
                let assets = self.registries.clone();
                resetter.reset(ComponentEventContext {
                    registries: &assets,
                    scene: self,
                    game_object,
                });
            }
        }
    }

    /// Returns the immutable Legion entry for `game_object`.
    pub(crate) fn entry(&self, game_object: GameObject) -> Option<EntryRef<'_>> {
        self.world.entry_ref(game_object.entity).ok()
    }

    /// A read-only view of the scene's object names, ids and hierarchy.
    pub fn view(&self) -> SceneView<'_> {
        SceneView::whole(&self.world.0, &self.graph, &self.store, self.root)
    }

    /// Returns the Legion world for read-only queries. Component writes go
    /// through the scene's write methods instead.
    pub fn world(&self) -> &World {
        &self.world.0
    }

    /// Returns the mutable Legion entry for `game_object`.
    ///
    /// This and the world split in [`Scene::edit_component_dyn`] are the only
    /// places mutable component storage is handed out. Their callers are the
    /// scene's write methods, which report each write through
    /// [`Scene::component_written`]; any other caller would let a write go
    /// unobserved.
    fn entry_mut(&mut self, game_object: GameObject) -> Option<Entry<'_>> {
        self.world.0.entry(game_object.entity)
    }

    /// Runs `writer` against `game_object`'s mutable entry and, when it returns
    /// `Some`, reports a write of `component` to the scene.
    fn write_entry<R>(
        &mut self,
        game_object: GameObject,
        component: ComponentTypeId,
        writer: impl FnOnce(&mut Entry) -> Option<R>,
    ) -> Option<R> {
        let result = writer(&mut self.entry_mut(game_object)?)?;
        self.component_written(game_object, component);
        Some(result)
    }

    /// Observes that `game_object`'s `component` was added, written or
    /// removed. Every component write in the scene ends here, and each one is
    /// recorded in the scene's change set for consumers such as physics.
    fn component_written(&mut self, game_object: GameObject, component: ComponentTypeId) {
        self.changes.record(game_object.entity, component);
        if component == ComponentTypeId::of::<ComponentID>() {
            self.keep_identity(game_object);
        } else if component == ComponentTypeId::of::<ComponentTransform>() {
            self.transforms.mark_dirty_subtree(game_object, &self.graph);
        } else if component == ComponentTypeId::of::<ComponentNetworkObject>() {
            self.refresh_network_object(game_object);
        }
    }

    /// Puts back the id `game_object` was spawned with after a write to its
    /// [`ComponentID`], so an object's id is read-only: names and visibility
    /// change through any write, the id never does. The id component is also
    /// restored if it went missing; the remove methods refuse to remove it.
    fn keep_identity(&mut self, game_object: GameObject) {
        let Some(id) = self.store.uuid(game_object.entity) else {
            return;
        };
        let Some(mut entry) = self.entry_mut(game_object) else {
            return;
        };
        match entry.get_component_mut::<ComponentID>() {
            Ok(component) => {
                if component.id != id {
                    log::warn!(
                        "Game object ids are read-only; keeping {id} instead of {}",
                        component.id
                    );
                    component.id = id;
                }
            }
            Err(_) => entry.add_component(ComponentID {
                id,
                ..Default::default()
            }),
        }
    }

    /// Reads component `T` from `game_object` and maps it through `reader`.
    pub fn read_component<T: Component, R, F: FnOnce(&T) -> R>(
        &self,
        game_object: GameObject,
        reader: F,
    ) -> Option<R> {
        let entry = self.entry(game_object);
        entry
            .as_ref()
            .and_then(|entry| entry.get_component::<T>().ok())
            .map(reader)
    }

    /// Mutates component `T` on `game_object` and returns what `writer`
    /// returns, or `None` when the object or component is missing.
    ///
    /// Transform writes invalidate cached world transforms for the object's
    /// subtree.
    pub fn write_component<T: Component + 'static, R>(
        &mut self,
        game_object: GameObject,
        writer: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.write_entry(game_object, ComponentTypeId::of::<T>(), |entry| {
            entry.get_component_mut::<T>().ok().map(writer)
        })
    }

    /// Mutates the component of `component`'s type on `game_object`, for
    /// callers that only know the type at runtime, such as through the
    /// component registry's prototypes. Returns `None` when the object or
    /// component is missing.
    pub fn write_component_dyn<R>(
        &mut self,
        game_object: GameObject,
        component: &dyn Component,
        writer: impl FnOnce(&mut dyn Component) -> R,
    ) -> Option<R> {
        self.write_entry(game_object, component.component_type_id(), |entry| {
            component.get_instance_mut(entry).map(writer)
        })
    }

    /// Lets `editor` change the component of `component`'s type on
    /// `game_object` while it reads the rest of the scene, as an inspector
    /// does. The scene reports a write only when `editor` says it wrote the
    /// component, so showing a component without editing it is not a write.
    /// Returns what `editor` returned, or `None` when the object or component
    /// is missing.
    ///
    /// The scene's world is split by component type while `editor` runs: it
    /// holds the component in place through the half that allows only the
    /// component's type, and reads the scene through a [`SceneView`] of the
    /// other half, which cannot read that type on any object. Nothing is
    /// copied or moved, so an idle frame costs no serialization. When `editor`
    /// panics, the component stays as `editor` left it and no write is
    /// reported.
    pub fn edit_component_dyn<R>(
        &mut self,
        game_object: GameObject,
        component: &dyn Component,
        editor: impl FnOnce(&mut dyn Component, &SceneView) -> ComponentEdit<R>,
    ) -> Option<R> {
        let edit = {
            let (mut edited, rest) = component.split_world(&mut self.world.0);
            let entry = edited.entry_mut(game_object.entity).ok()?;
            let instance = component.split_instance_mut(entry)?;
            let view = SceneView::split(rest, &self.graph, &self.store, self.root);
            editor(instance, &view)
        };
        if edit.written {
            // A write to the object's ComponentID may have its id put back
            // here; callers that need the written value read it afterwards.
            self.component_written(game_object, component.component_type_id());
        }
        Some(edit.value)
    }

    /// Removes component `T` from `game_object`. Removing the object's
    /// [`ComponentID`] is refused, since every object keeps its identity.
    pub fn remove_component<T: Component + 'static>(&mut self, game_object: GameObject) {
        if ComponentTypeId::of::<T>() == ComponentTypeId::of::<ComponentID>() {
            log::warn!("Game objects keep their ComponentID; not removing it");
            return;
        }
        self.write_entry(game_object, ComponentTypeId::of::<T>(), |entry| {
            entry.remove_component::<T>();
            Some(())
        });
    }

    /// Removes the component with reflected type UUID `type_uuid` from
    /// `game_object`. Returns whether the component type is registered and
    /// the object exists. Removing the object's [`ComponentID`] is refused
    /// and returns `false`, since every object keeps its identity.
    pub fn remove_component_dyn(&mut self, game_object: GameObject, type_uuid: Uuid) -> bool {
        if type_uuid == ComponentID::type_uuid() {
            log::warn!("Game objects keep their ComponentID; not removing it");
            return false;
        }
        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let Some(component) = component_registry.component(type_uuid) else {
            return false;
        };
        self.write_entry(game_object, component.component_type_id(), |entry| {
            component.remove_instance(entry);
            Some(())
        })
        .is_some()
    }

    /// Flushes pending deletions and prepares the physics scene for the next
    /// frame.
    pub fn prepare(&mut self) {
        self.flush_deletes();
        PhysicsContext::prepare(self);
    }

    /// Invokes one-time start hooks for all components currently in the scene.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;

        let registries = self.registries.clone();
        let component_registry_ref = registries.components.clone();
        let component_registry = component_registry_ref.read();
        let game_objects: Vec<GameObject> = <Entity>::query()
            .iter(&self.world.0)
            .filter_map(|entity| self.game_object_from_entity(*entity))
            .filter(|go| *go != self.root)
            .collect();

        for (_type_uuid, starter) in component_registry.components_with_start() {
            for &game_object in &game_objects {
                starter.start(ComponentEventContext {
                    registries: &registries,
                    scene: self,
                    game_object,
                });
            }
        }
    }

    /// Advances physics and all registered component update hooks for one
    /// frame.
    pub fn update(
        &mut self,
        registries: &ReadOnlyRegistryContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        {
            let time = resources.time();
            let physics_config = resources.physics_configuration();
            PhysicsContext::update(self, time, physics_config);
        }
        let component_registry = registries.components.read();

        // Collect once — each updater's read_component bails early for entities
        // that don't have its component type. The root carries no behaviour.
        let game_objects: Vec<GameObject> = <Entity>::query()
            .iter(&self.world.0)
            .filter_map(|e| self.game_object_from_entity(*e))
            .filter(|go| *go != self.root)
            .collect();

        for (_type_uuid, updater) in component_registry.components_with_update() {
            for &game_object in &game_objects {
                updater.update(
                    ComponentEventContext {
                        registries,
                        scene: self,
                        game_object,
                    },
                    resources,
                    input,
                );
            }
        }
    }

    /// Applies all queued deletions immediately.
    ///
    /// Handles that no longer exist, the root, and handles whose ancestor is
    /// also queued are skipped, so each queued subtree is removed exactly once
    /// regardless of queue order.
    pub fn flush_deletes(&mut self) {
        let queued: HashSet<GameObject> = self
            .store
            .drain_deletions()
            .into_iter()
            .filter(|go| *go != self.root && self.graph.game_object_from_node(go.node) == Some(*go))
            .collect();
        let roots: Vec<GameObject> = queued
            .iter()
            .copied()
            .filter(|go| {
                // Collect ancestors until the chain ends, repeats, or returns to
                // `go` itself (so `go` is never its own ancestor).
                let mut visited = HashSet::from([*go]);
                let mut chain = Vec::new();
                let mut on_cycle = false;
                let mut current = self.parent(*go);
                while let Some(ancestor) = current {
                    if ancestor == *go {
                        on_cycle = true;
                        break;
                    }
                    if !visited.insert(ancestor) {
                        break;
                    }
                    chain.push(ancestor);
                    current = self.parent(ancestor);
                }
                let mut queued_ancestors = chain.iter().filter(|a| queued.contains(*a));
                if on_cycle {
                    // Only the queued cycle member with the lowest node index
                    // stays a root so the cycle is deleted exactly once.
                    queued_ancestors.all(|a| a.node.index() > go.node.index())
                } else {
                    queued_ancestors.next().is_none()
                }
            })
            .collect();
        for game_object in roots {
            for go in self.graph.detach_subtree(game_object) {
                self.network_objects.remove(go);
                self.world.0.remove(go.entity);
                self.changes.forget(go.entity);
                self.store.remove(go.entity);
                self.transforms.remove(go.node);
            }
        }
    }

    /// Iterates every non-root object in the scene.
    pub fn objects(&self) -> impl Iterator<Item = GameObject> + '_ {
        self.view().objects()
    }

    /// Returns the display name stored in [`ComponentID`] for `game_object`.
    pub fn name(&self, game_object: GameObject) -> String {
        self.view().name(game_object).unwrap_or_default()
    }

    /// Returns whether `game_object` is locally marked visible.
    pub fn is_visible(&self, game_object: GameObject) -> bool {
        self.entry(game_object)
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.visible))
            .unwrap_or(true)
    }

    /// Returns whether `game_object` and every parent in its hierarchy are visible.
    pub fn is_visible_in_hierarchy(&self, game_object: GameObject) -> bool {
        self.is_visible(game_object) && self.ancestors(game_object).all(|go| self.is_visible(go))
    }

    /// Returns the persistent UUID `game_object` was spawned with, which its
    /// [`ComponentID`] always holds, or the nil UUID for an unknown object.
    pub fn uuid(&self, game_object: GameObject) -> Uuid {
        self.view().uuid(game_object)
    }

    /// Returns the type ids of every component on `game_object`, or an empty
    /// list when the object does not exist. Compare them with
    /// [`ComponentInstance::component_type_id`](crate::component::ComponentInstance::component_type_id)
    /// to find which registered components the object carries.
    pub fn component_types(&self, game_object: GameObject) -> Vec<ComponentTypeId> {
        self.entry(game_object)
            .map(|entry| entry.archetype().layout().component_types().to_vec())
            .unwrap_or_default()
    }

    /// The write version of the component of `component`'s type on
    /// `game_object`. It changes on every write the scene observes, from any
    /// writer, and is 0 when the component was never written since the
    /// object was spawned. Compare it with an earlier reading to learn
    /// whether someone wrote the component since, without consuming the
    /// scene's change set.
    pub fn write_version(&self, game_object: GameObject, component: &dyn Component) -> u64 {
        self.changes
            .version(game_object.entity, component.component_type_id())
    }

    /// Reads the component of `component`'s type on `game_object` and maps it
    /// through `reader`, for callers that only know the type at runtime, such
    /// as through the component registry's prototypes. Returns `None` when the
    /// object or component is missing.
    pub fn read_component_dyn<R>(
        &self,
        game_object: GameObject,
        component: &dyn Component,
        reader: impl FnOnce(&dyn Component) -> R,
    ) -> Option<R> {
        let entry = self.entry(game_object)?;
        component.get_instance(&entry).map(reader)
    }

    /// Resolves a game object by persistent UUID.
    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.view().find(id)
    }

    /// Returns the direct parent of `game_object`, if any.
    pub fn parent(&self, game_object: GameObject) -> Option<GameObject> {
        self.view().parent(game_object)
    }

    /// Returns the UUID of `game_object`'s parent, if any.
    pub fn parent_uuid(&self, game_object: GameObject) -> Option<Uuid> {
        self.parent(game_object).map(|parent| self.uuid(parent))
    }

    /// Iterates direct children of `game_object` without preserving authored
    /// order.
    pub fn children(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        self.graph.children(game_object)
    }

    /// Returns a detached walker for incremental traversal of direct children.
    pub fn children_walker(&self, game_object: GameObject) -> scene_graph::WalkChildren {
        self.graph.children_walker(game_object)
    }

    /// Iterates direct children of `game_object` in sibling order.
    pub fn children_ordered(
        &self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + '_ {
        self.graph.children_ordered(game_object)
    }

    /// Returns the child stored at `index` under `game_object`.
    pub fn child_at(&self, game_object: GameObject, index: i32) -> Option<GameObject> {
        self.graph.child_at(game_object, index)
    }

    /// Returns `true` when `game_object` belongs to the subtree rooted at
    /// `parent`.
    pub fn is_descendant(&self, parent: GameObject, game_object: GameObject) -> bool {
        self.graph.is_descendant(parent, game_object)
    }

    /// Returns the local transform stored on `game_object`.
    ///
    /// When the object has no [`ComponentTransform`], the identity transform is
    /// returned.
    pub fn transform(&self, game_object: GameObject) -> Transform {
        let Some(entry) = self.entry(game_object) else {
            return Default::default();
        };
        let Ok(c_transform) = entry.get_component::<ComponentTransform>() else {
            return Default::default();
        };
        c_transform.transform
    }

    /// Sets the local transform matrix for `game_object`.
    pub fn set_transform(&mut self, game_object: GameObject, matrix: &Mat4) {
        self.write_component::<ComponentTransform, _>(game_object, |c_transform| {
            c_transform.transform.set_local_matrix(matrix);
        });
    }

    /// Sets `game_object`'s world transform by converting through its parent
    /// space.
    pub fn set_world_transform(&mut self, game_object: GameObject, matrix: impl Into<Mat4>) {
        let parent_transform = self.parent(game_object).map_or(Mat4::identity(), |go| {
            self.world_transform(go).inverse_matrix()
        });
        self.set_transform(game_object, &(parent_transform * matrix.into()));
    }

    /// Returns the cached or computed world transform for `game_object`.
    pub fn world_transform(&self, game_object: GameObject) -> Transform {
        self.transforms
            .world_transform(game_object, &self.world, &self.graph)
    }

    /// Returns `game_object`'s transform relative to `parent`.
    pub fn transform_relative_to(&self, game_object: GameObject, parent: GameObject) -> Transform {
        let transform = self.world_transform(game_object);
        let parent_transform = self.world_transform(parent);
        (parent_transform.inverse_matrix() * transform.matrix()).into()
    }

    fn map_has_component<T: Component>(&self, game_object: GameObject) -> Option<GameObject> {
        self.entry(game_object)
            .and_then(|e| e.get_component::<T>().ok().map(|_| game_object))
    }

    /// Iterates `game_object` and every descendant in breadth-first order.
    pub fn descendants(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        self.graph.descendants(game_object)
    }

    /// Iterates descendants that contain component `T`.
    pub fn descendants_with<T: Component>(
        &self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + '_ {
        self.descendants(game_object)
            .filter_map(|go| self.map_has_component::<T>(go))
    }

    /// Iterates ancestors from the direct parent upward.
    pub fn ancestors(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        self.graph.ancestors(game_object)
    }

    /// Returns the first ancestor that contains component `T`.
    pub fn ancestor_with<T: Component>(&self, game_object: GameObject) -> Option<GameObject> {
        self.ancestors(game_object)
            .find_map(|go| self.map_has_component::<T>(go))
    }

    /// Finds the game object carrying network object `network_object_id`.
    ///
    /// Lookups go through an index the scene keeps up to date on every
    /// [`ComponentNetworkObject`] add, write or removal and on deletions. A
    /// hit is still checked against the object's component, and a miss or
    /// stale hit falls back to one scan of the scene that rebuilds the index.
    pub fn find_network_object(
        &mut self,
        network_object_id: NetworkObjectId,
    ) -> Option<GameObject> {
        if let Some(game_object) = self.network_objects.get(network_object_id) {
            if self.network_id(game_object) == Some(network_object_id) {
                return Some(game_object);
            }
        }
        self.rebuild_network_index();
        self.network_objects.get(network_object_id)
    }

    /// Returns the indexed game object for `network_object_id` without
    /// checking or repairing the index.
    pub(crate) fn indexed_network_object(
        &self,
        network_object_id: NetworkObjectId,
    ) -> Option<GameObject> {
        self.network_objects.get(network_object_id)
    }

    /// Returns the network object id of `game_object`, if it has one.
    fn network_id(&self, game_object: GameObject) -> Option<NetworkObjectId> {
        self.read_component::<ComponentNetworkObject, _, _>(game_object, |c| c.id)
    }

    /// Brings the network object index up to date with `game_object`'s
    /// current [`ComponentNetworkObject`], after it was added, written or
    /// removed. [`Scene::component_written`] calls it for every such write.
    fn refresh_network_object(&mut self, game_object: GameObject) {
        // Inserting drops an old id of this object; other claimants of the
        // id stay listed behind it.
        match self.network_id(game_object) {
            Some(network_id) => self.network_objects.insert(network_id, game_object),
            None => self.network_objects.remove(game_object),
        }
    }

    /// Rebuilds the network object index from every object in the scene.
    fn rebuild_network_index(&mut self) {
        let network_objects = self
            .objects()
            .filter_map(|go| self.network_id(go).map(|id| (id, go)))
            .collect();
        self.network_objects = network_objects;
    }

    /// Returns whether the local peer owns `game_object` for networking
    /// purposes.
    ///
    /// Objects without a [`ComponentNetworkObject`] are treated as locally
    /// owned.
    pub fn is_owner(&self, game_object: GameObject, network: &Network) -> bool {
        let Some(entry) = self.entry(game_object) else {
            return false;
        };
        let Ok(c_netobj) = entry.get_component::<ComponentNetworkObject>() else {
            return true;
        };
        c_netobj.is_owner(network)
    }
}
