use bimap::BiHashMap;
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use nalgebra_glm::Mat4;
use serde::de::DeserializeSeed;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
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
use crate::net::{ComponentNetworkObject, Network};
use crate::physics::PhysicsContext;
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::resource::ResourceMap;
use crate::scene::game_object_store::GameObjectStore;
use crate::scene::scene_graph::SceneGraph;
use crate::scene::transform_cache::TransformCache;
use crate::scene::{GameObjectRef, Prefab};
use crate::try_all;
use crate::utils::{ContextSeed, TypeUuid};

use super::scene_graph::{self, SiblingDir};

/// Lightweight handle to a game object stored inside a [`Scene`].
///
/// The handle contains both the Legion entity and the scene-graph node index so
/// hierarchy and ECS lookups can stay cheap. Persist this handle only for the
/// lifetime of the scene instance; for serialized references use
/// [`GameObjectRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameObject {
    /// Internal scene-graph node for hierarchy operations.
    pub node: petgraph::stable_graph::NodeIndex,
    /// Legion entity that stores the object's components.
    pub entity: Entity,
}

/// Serializable representation of a scene or subtree.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    /// Component payloads keyed by game object UUID and then component type
    /// UUID.
    pub components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    /// Parent-child relationships keyed by parent UUID.
    pub hierarchy: HashMap<Uuid, Vec<Uuid>>,
}

/// Detached snapshot of a scene that can be restored later against a registry
/// context.
#[derive(Clone)]
pub struct SceneSnapshot {
    data: SceneData,
}

/// Runtime scene containing ECS state, hierarchy state, and physics state for a
/// collection of game objects.
#[derive(TypeUuid)]
#[uuid = "9946a2e7-e022-447e-8e60-528da548087f"]
pub struct Scene {
    /// Legion world that stores all scene components.
    pub world: World,
    /// Physics state coupled to the scene.
    pub physics: PhysicsContext,
    pub(crate) graph: SceneGraph,
    pub(crate) store: GameObjectStore,
    pub(crate) transforms: TransformCache,
    root: GameObject,
    camera: Option<GameObject>,
    registries: ReadOnlyRegistryContext,
}

impl Scene {
    /// Creates an empty scene with a generated root object.
    pub fn new(assets: ReadOnlyRegistryContext) -> Self {
        let mut world: World = Default::default();
        let mut graph = SceneGraph::default();
        let entity = world.push(());
        let node = graph.add_node(entity);
        let root = GameObject { node, entity };
        let id = Uuid::new_v4();
        world.entry(entity).unwrap().add_component(ComponentID {
            id,
            name: String::from("Root"),
            visible: true,
        });
        let mut store = GameObjectStore::default();
        store.register_uuid(id, root);
        Self {
            world,
            physics: Default::default(),
            graph,
            store,
            transforms: TransformCache::default(),
            root,
            camera: Default::default(),
            registries: assets,
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
        for (_, components) in value.components {
            let game_object = scene.new_game_object(None);
            for (component_id, data) in components {
                try_all!(
                    None => continue;
                    let component = registry.component(component_id);
                    let instance = component.deserialize(&data);
                    let mut entry = scene.entry_mut(game_object);
                );
                let _ = component.bind_instance(&mut entry, instance);
            }
            let mut id = None;
            if let Some(entry) = scene.entry(game_object) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                scene.store.register_uuid(id, game_object);
            }
        }
        for (parent_id, children) in value.hierarchy {
            try_all!(
                None => continue;
                let parent = scene.find(parent_id);
            );
            for child_id in children {
                try_all!(
                    None => continue;
                    let child = scene.find(child_id);
                );
                scene.set_parent(child, Some(parent));
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
        let world = &scene.world;
        let mut query = <(Entity, &ComponentID)>::query();
        let mut data = Default::default();
        for (entity, id) in query.iter(world) {
            let Some(game_object) = scene.game_object_from_entity(*entity) else {
                continue;
            };
            scene.serialize_game_object(game_object, id.id, &mut data);
        }
        data
    }
}

impl From<(&Scene, GameObject)> for SceneData {
    fn from((scene, game_object): (&Scene, GameObject)) -> Self {
        let mut data = Default::default();
        let game_objects = scene.graph.bfs_from(game_object.node);
        let subtree = game_objects.iter().copied().collect::<HashSet<_>>();
        game_objects.into_iter().for_each(|game_object| {
            let Some(entry) = scene.entry(game_object) else {
                return;
            };
            let Ok(id) = entry.get_component::<ComponentID>() else {
                return;
            };
            scene.serialize_game_object_filtered(game_object, id.id, &mut data, Some(&subtree));
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
            try_all!(
                None => break 'insert_hierarchy;
                let parent = self.parent(game_object);
                let entry = self.entry(parent);
                let parent_id = entry.get_component::<ComponentID>().ok();
            );
            if parent_filter.is_some_and(|filter| !filter.contains(&parent)) {
                break 'insert_hierarchy;
            }
            data.hierarchy
                .entry(parent_id.id)
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
        let game_object = self.new_game_object(parent);
        self.store.register_uuid(id.id, game_object);
        self.bind_component(game_object, id);
        self.bind_component(game_object, ComponentTransform::default());
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

    /// Instantiates `prefab` into the scene and remaps any embedded
    /// [`GameObjectRef`] values to the newly created objects.
    pub fn instantiate_prefab(
        &mut self,
        prefab: &Prefab,
        parent: Option<GameObject>,
    ) -> Option<GameObject> {
        let mut id_mapping = prefab
            .data
            .components
            .iter()
            .map(|(game_object_id, _id)| (*game_object_id, Uuid::new_v4()))
            .collect::<BiHashMap<_, _>>();

        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let type_registry_ref = self.registries.types.clone();
        let type_registry = type_registry_ref.read();
        for (game_object_id, _components) in prefab.data.components.iter() {
            let game_object = self.new_game_object(None);
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).unwrap();
            self.store.register_uuid(new_game_object_id, game_object);
        }

        for (game_object_id, components) in prefab.data.components.iter() {
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).unwrap();
            let game_object = self.find(new_game_object_id).unwrap();
            for (component_id, data) in components {
                try_all!(
                    None => continue;
                    let TypeInfo::Struct(struct_info) = type_registry.type_info_by_id(*component_id);
                    let component = component_registry.component(*component_id);
                    let mut instance = component.deserialize(data);
                    let mut entry = self.entry_mut(game_object);
                );
                for (name, field) in &struct_info.fields {
                    try_all!(
                        None => continue;
                        let id = field.get::<GameObjectRef>(&*instance).map(|r| r.id());
                        let target_id = id_mapping.get_by_left(&id);
                    );
                    log::trace!("{} - {}", name, field.name);
                    field
                        .set(&mut *instance, GameObjectRef::new(*target_id))
                        .unwrap();
                }
                let _ = component.bind_instance(&mut entry, instance);
            }
            if let Some(mut entry) = self.entry_mut(game_object) {
                if let Ok(c_id) = entry.get_component_mut::<ComponentID>() {
                    c_id.id = new_game_object_id;
                }
            }
        }

        for (parent_id, children) in prefab.data.hierarchy.iter() {
            try_all!(
                None => continue;
                let new_parent_id = id_mapping.get_by_left(parent_id);
                let parent = self.find(*new_parent_id);
            );
            for child_id in children {
                try_all!(
                    None => continue;
                    let new_child_id = id_mapping.get_by_left(child_id);
                    let child = self.find(*new_child_id);
                );
                self.set_parent(child, Some(parent));
            }
        }

        try_all!(
            None => return None;
            let prefab_uuid = id_mapping.get_by_left(&prefab.root);
            let game_object = self.find(*prefab_uuid);
        );

        if let Some(parent) = parent {
            self.set_parent(game_object, Some(parent));
        }

        Some(game_object)
    }

    /// Reparents `game_object` under `parent`, appending it after existing
    /// siblings.
    pub fn set_parent(&mut self, game_object: GameObject, parent: Option<GameObject>) {
        self.set_parent_with_sibling(game_object, parent, None);
    }

    /// Reparents `game_object` and optionally inserts it relative to `sibling`.
    pub fn set_parent_with_sibling(
        &mut self,
        game_object: GameObject,
        parent: Option<GameObject>,
        sibling: Option<(GameObject, SiblingDir)>,
    ) {
        let parent = parent.unwrap_or(self.root);
        self.graph.set_parent(game_object, parent, sibling);
        self.transforms.mark_dirty_subtree(game_object, &self.graph);
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
            .iter(&self.world)
            .filter_map(|(e, _t, c)| self.game_object_from_entity(*e).map(|go| (go, c)))
            .find(|(go, c)| {
                if let Some(camera) = &self.camera {
                    go == camera
                } else {
                    c.enabled
                }
            })
            .map(|(go, c)| (go, c))
    }

    pub(crate) fn new_game_object(&mut self, parent: Option<GameObject>) -> GameObject {
        let entity = self.world.push(());
        let node = self.graph.add_node(entity);
        let game_object = GameObject { node, entity };
        self.store.register_entity(entity, node);
        let parent_node = parent.unwrap_or(self.root);
        let edge_index = self.graph.next_edge_index(parent_node);
        self.graph.add_edge(parent_node.node, node, edge_index);
        game_object
    }

    /// Adds a concrete component instance to `game_object`.
    pub fn add_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) {
        self.bind_component(game_object, component);
    }

    pub(crate) fn bind_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) {
        self.entry_mut(game_object)
            .map(|mut e| e.add_component(component));
        if TypeId::of::<T>() == TypeId::of::<ComponentTransform>() {
            self.transforms.mark_dirty_subtree(game_object, &self.graph);
        }
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
        let Some(mut entry) = self.entry_mut(game_object) else {
            return;
        };
        let result = component.bind_instance(&mut entry, default_instance);
        if result {
            drop(entry);
            if type_uuid == ComponentTransform::type_uuid() {
                self.transforms.mark_dirty_subtree(game_object, &self.graph);
            }
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

    /// Returns a raw mutable pointer to a component instance.
    ///
    /// # Safety
    ///
    /// The returned pointer is tied to the current contents of the Legion
    /// world. Callers must not hold it across scene mutations that could move or
    /// remove the component, and they must uphold Rust aliasing rules manually.
    pub unsafe fn get_component_ptr(
        &mut self,
        game_object: GameObject,
        component: &dyn Component,
    ) -> Option<*mut dyn Component> {
        self.entry_mut(game_object).and_then(|mut entry| {
            component
                .get_instance_mut(&mut entry)
                .map(|instance| instance as *mut dyn Component)
        })
    }

    /// Returns the immutable Legion entry for `game_object`.
    pub fn entry(&self, game_object: GameObject) -> Option<EntryRef<'_>> {
        self.world.entry_ref(game_object.entity).ok()
    }

    /// Returns the mutable Legion entry for `game_object`.
    pub fn entry_mut(&mut self, game_object: GameObject) -> Option<Entry<'_>> {
        self.world.entry(game_object.entity)
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

    /// Mutates component `T` on `game_object`.
    ///
    /// Transform writes invalidate cached world transforms for the object's
    /// subtree.
    pub fn write_component<T: Component + 'static, F: FnOnce(&mut T)>(
        &mut self,
        game_object: GameObject,
        writer: F,
    ) -> Option<()> {
        let mut entry = self.entry_mut(game_object);
        let result = entry
            .as_mut()
            .and_then(|entry| entry.get_component_mut::<T>().ok())
            .map(writer);
        drop(entry);
        if result.is_some() && TypeId::of::<T>() == TypeId::of::<ComponentTransform>() {
            self.transforms.mark_dirty_subtree(game_object, &self.graph);
        }
        result
    }

    /// Flushes pending deletions and prepares the physics scene for the next
    /// frame.
    pub fn prepare(&mut self) {
        self.flush_deletes();
        PhysicsContext::prepare(self);
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
        // that don't have its component type.
        let game_objects: Vec<GameObject> = <Entity>::query()
            .iter(&self.world)
            .filter_map(|e| self.game_object_from_entity(*e))
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
    pub fn flush_deletes(&mut self) {
        for game_object in self.store.drain_deletions() {
            let parent = self.parent(game_object).unwrap_or(self.root);
            let index = self
                .graph
                .index_in_parent(parent, game_object, SiblingDir::Before)
                .unwrap();
            self.graph.shift_edge_weights(parent, index, -1);
            let to_delete: Vec<_> = self
                .graph
                .bfs_from(game_object.node)
                .into_iter()
                .rev()
                .collect();
            for go in to_delete {
                let uuid = self.uuid(go);
                self.world.remove(go.entity);
                self.store.remove(uuid, go.entity);
                self.graph.remove_node(go.node);
            }
        }
    }

    /// Iterates every non-root object in the scene.
    pub fn objects(&self) -> impl Iterator<Item = GameObject> + '_ {
        self.graph.objects(self.root)
    }

    /// Returns the display name stored in [`ComponentID`] for `game_object`.
    pub fn name(&self, game_object: GameObject) -> String {
        self.entry(game_object)
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
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

    /// Returns the persistent UUID stored in [`ComponentID`] for `game_object`.
    pub fn uuid(&self, game_object: GameObject) -> Uuid {
        self.entry(game_object)
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    /// Resolves a game object by persistent UUID.
    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.store.find(id)
    }

    /// Returns the direct parent of `game_object`, if any.
    pub fn parent(&self, game_object: GameObject) -> Option<GameObject> {
        self.graph.parent(game_object)
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
        let Some(mut entry) = self.entry_mut(game_object) else {
            return;
        };
        let Ok(c_transform) = entry.get_component_mut::<ComponentTransform>() else {
            return;
        };
        c_transform.transform.set_local_matrix(matrix);
        self.transforms.mark_dirty_subtree(game_object, &self.graph);
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

    /// Invalidates every cached world transform in the scene.
    pub fn clear_transform_cache(&self) {
        self.transforms.clear();
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
