use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use glm::Mat4;
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use petgraph::prelude::{EdgeRef, StableGraph};
use petgraph::stable_graph::{DefaultIx, NodeIndex, WalkNeighbors};
use petgraph::visit::{Bfs, Dfs, Reversed, Walker};
use petgraph::Direction;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::class_registry::ClassRegistry;
use crate::component::{Component, ComponentTransform};
use crate::component::{ComponentCamera, ComponentID};
use crate::core::Time;
use crate::input::Input;
use crate::math::Transform;
use crate::physics::{PhysicsConfiguration, PhysicsContext};
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::ReflectDefault;
use crate::scene::Prefab;
use crate::utils::TypeUuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameObject {
    pub node: NodeIndex,
    pub entity: Entity,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    pub components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    pub hierarchy: HashMap<Uuid, Uuid>,
}

#[derive(TypeUuid)]
#[uuid = "9946a2e7-e022-447e-8e60-528da548087f"]
pub struct Scene {
    pub world: World,
    pub physics: PhysicsContext,
    uuid_map: HashMap<Uuid, GameObject>,
    entity_map: HashMap<Entity, NodeIndex>,
    entity_arena: StableGraph<Entity, i32>,
    root: GameObject,
    transform_cache: RwLock<HashMap<NodeIndex, Transform>>,
    camera: Option<GameObject>,
    objects_to_delete: HashSet<GameObject>,
    new_index: usize,
}

pub struct WalkChildren {
    walker: WalkNeighbors<DefaultIx>,
}

pub enum SiblingDir {
    Before,
    After,
}

impl WalkChildren {
    pub fn next(&mut self, scene: &Scene) -> Option<GameObject> {
        self.walker
            .next_node(&scene.entity_arena)
            .and_then(|node| scene.get_game_object_from_node(node))
    }
}

impl Default for Scene {
    fn default() -> Self {
        let mut world: World = Default::default();
        let mut entity_arena: StableGraph<Entity, i32> = Default::default();
        let entity = world.push(());
        let node = entity_arena.add_node(entity);
        let root = GameObject { node, entity };
        let id = Uuid::new_v4();
        world.entry(entity).unwrap().add_component(ComponentID {
            id,
            name: String::from("Root"),
            visible: true,
        });
        Self {
            world,
            physics: Default::default(),
            uuid_map: [(id, root)].into(),
            entity_map: Default::default(),
            entity_arena,
            root,
            transform_cache: Default::default(),
            camera: Default::default(),
            objects_to_delete: Default::default(),
            new_index: 0,
        }
    }
}

impl Asset for Scene {
    fn get_file_extensions() -> &'static [&'static str]
    where
        Self: Sized,
    {
        &["cxscene"]
    }

    fn from_file(path: &Path) -> Result<LoadedAsset<Self>, AssetError>
    where
        Self: Sized,
    {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| AssetError::LoadError)?;

        let reader = BufReader::new(file);
        Ok(LoadedAsset::new(
            serde_json::from_reader(reader).map_err(|_| AssetError::LoadError)?,
        ))
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

impl<'de> Deserialize<'de> for Scene {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Scene::from(SceneData::deserialize(deserializer)?))
    }
}

impl Clone for Scene {
    fn clone(&self) -> Self {
        let data: SceneData = self.into();
        data.into()
    }
}

impl From<SceneData> for Scene {
    fn from(value: SceneData) -> Self {
        let mut scene = Self::default();
        for (_, components) in value.components {
            let game_object = scene.new_game_object(None);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(component_id) {
                    if let Some(instance) = component.deserialize(data) {
                        if let Some(mut entry) = scene.entry_mut(game_object) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = scene.entry(game_object) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                scene.uuid_map.insert(id, game_object);
            }
        }
        for (id, parent) in value.hierarchy {
            if let Some(game_object) = scene.get_game_object_by_uuid(id) {
                if let Some(parent) = scene.get_game_object_by_uuid(parent) {
                    scene.set_parent(game_object, Some(parent));
                }
            }
        }
        scene
    }
}

impl From<&Scene> for SceneData {
    fn from(scene: &Scene) -> Self {
        let world = &scene.world;
        let mut query = <(Entity, &ComponentID)>::query();
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();
        for (entity, id) in query.iter(world) {
            if let Some(game_object) = scene.get_game_object_from_entity(*entity) {
                if let Some(parent) = scene.get_parent_game_object(game_object) {
                    if let Some(entry) = scene.entry(parent) {
                        if let Ok(parent_id) = entry.get_component::<ComponentID>() {
                            hierarchy.insert(id.id, parent_id.id);
                        }
                    }
                }
                for (component_id, component) in ClassRegistry::get().components_uuid() {
                    if let Some(entry) = scene.entry(game_object) {
                        if let Some(instance) = component.get_instance(&entry) {
                            if let Some(value) = instance.serialize() {
                                components
                                    .entry(id.id)
                                    .or_default()
                                    .insert(*component_id, value);
                            }
                        }
                    }
                }
            }
        }
        SceneData {
            hierarchy,
            components,
        }
    }
}

impl From<(&Scene, GameObject)> for SceneData {
    fn from((scene, game_object): (&Scene, GameObject)) -> Self {
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();

        std::iter::once(game_object.node)
            .chain(Bfs::new(&scene.entity_arena, game_object.node).iter(&scene.entity_arena))
            .filter_map(|c| scene.get_game_object_from_node(c))
            .for_each(|game_object| {
                if let Some(entry) = scene.entry(game_object) {
                    if let Ok(id) = entry.get_component::<ComponentID>() {
                        if let Some(parent) = scene.get_parent_game_object(game_object) {
                            if let Some(entry) = scene.entry(parent) {
                                if let Ok(parent_id) = entry.get_component::<ComponentID>() {
                                    hierarchy.insert(id.id, parent_id.id);
                                }
                            }
                        }
                        for (component_id, component) in ClassRegistry::get().components_uuid() {
                            if let Some(entry) = scene.entry(game_object) {
                                if let Some(instance) = component.get_instance(&entry) {
                                    if let Some(value) = instance.serialize() {
                                        components
                                            .entry(id.id)
                                            .or_default()
                                            .insert(*component_id, value);
                                    }
                                }
                            }
                        }
                    }
                }
            });

        SceneData {
            hierarchy,
            components,
        }
    }
}

impl Scene {
    pub(crate) fn get_game_object_from_node(&self, node: NodeIndex) -> Option<GameObject> {
        self.entity_arena
            .node_weight(node)
            .map(|e| GameObject { node, entity: *e })
    }

    pub(crate) fn get_game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.entity_map.get(&entity).map(|node| GameObject {
            node: *node,
            entity,
        })
    }
}

#[allow(unused)]
impl Scene {
    pub fn root(&self) -> GameObject {
        self.root
    }

    pub fn root_id(&self) -> Uuid {
        self.get_game_object_uuid(self.root)
    }

    pub fn root_objects<'a>(&'a self) -> impl Iterator<Item = GameObject> + 'a {
        self.get_children_ordered(self.root)
    }

    // TODO: Refactor this to call Scene::new_game_object in order to avoid code duplication
    pub fn create_game_object(
        &mut self,
        id: Option<ComponentID>,
        parent: Option<GameObject>,
    ) -> GameObject {
        let is_default_id = id.is_none();
        let mut id = id.unwrap_or_default();
        if is_default_id {
            let number = if self.new_index != 0 {
                format!(" ({})", self.new_index)
            } else {
                "".into()
            };
            id.name = format!("Game Object{}", number);
            self.new_index += 1;
        }
        let game_object = self.new_game_object(parent);
        self.uuid_map.insert(id.id, game_object);
        self.bind_component(game_object, id);
        self.bind_component(game_object, ComponentTransform::default());
        game_object
    }

    pub fn delete_game_object(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    pub fn create_prefab(&self, game_object: GameObject) -> Prefab {
        let data: SceneData = (self, game_object).into();

        Prefab {
            data: data.clone(),
            scene: data.into(),
        }
    }

    pub fn instantiate_prefab(&mut self, prefab: &Prefab, parent: Option<GameObject>) {
        let root_node = prefab.scene.game_objects().next().unwrap();

        for (_, components) in prefab.data.components.iter() {
            let game_object = self.new_game_object(None);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(*component_id) {
                    if let Some(instance) = component.deserialize(data.clone()) {
                        if let Some(mut entry) = self.entry_mut(game_object) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = self.entry(game_object) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                self.uuid_map.insert(id, game_object);
            }
        }

        for (id, parent) in prefab.data.hierarchy.iter() {
            if let Some(game_object) = self.get_game_object_by_uuid(*id) {
                if let Some(parent) = self.get_game_object_by_uuid(*parent) {
                    self.set_parent(game_object, Some(parent));
                }
            }
        }

        // for (id, _) in prefab.data.components.iter() {
        //     if let Some(node) = self.get_node_by_uuid(*id) {
        //         self.world
        //             .entry(self.get_entity(node))
        //             .unwrap()
        //             .get_component_mut::<ComponentID>()
        //             .unwrap()
        //             .id = Uuid::new_v4();
        //     }
        // }

        if let Some(parent) = parent {
            if let Some(game_object) =
                self.get_game_object_by_uuid(prefab.scene.get_game_object_uuid(root_node))
            {
                self.set_parent(game_object, Some(parent));
            }
        }
    }

    pub fn set_parent(&mut self, game_object: GameObject, parent: Option<GameObject>) {
        self.set_parent_with_sibling(game_object, parent, None);
    }

    pub fn set_parent_with_sibling(
        &mut self,
        game_object: GameObject,
        parent: Option<GameObject>,
        sibling: Option<(GameObject, SiblingDir)>,
    ) {
        let parent = parent.unwrap_or(self.root);
        let mut insert_index = None;
        if let Some((sibling, dir)) = sibling {
            if let Some(index) = self.get_index_in_parent(parent, sibling, dir) {
                if let Some(current) =
                    self.get_index_in_parent(parent, game_object, SiblingDir::Before)
                {
                    // Same parent, just swap edge weights and done
                    self.swap_edge_weights(parent, current, index);
                    return;
                } else {
                    // Adding a new edge, shift greater weights by +1
                    self.shift_edge_weights(parent, index, 1);
                }
                insert_index = Some(index);
            }
        }

        if let Some((parent, edge)) = self.get_parent_game_object(game_object).and_then(|parent| {
            self.entity_arena
                .find_edge(parent.node, game_object.node)
                .map(|edge| (parent, edge))
        }) {
            // Removing an edge, shift greater weights by -1
            let index = self.entity_arena[edge];
            self.entity_arena.remove_edge(edge);
            self.shift_edge_weights(parent, index, -1);
        }
        let insert_index = insert_index.unwrap_or_else(|| self.next_edge_index(Some(parent)));
        self.entity_arena
            .add_edge(parent.node, game_object.node, insert_index);
    }

    pub fn get_index_in_parent(
        &self,
        parent: GameObject,
        sibling: GameObject,
        dir: SiblingDir,
    ) -> Option<i32> {
        let edge = self
            .entity_arena
            .edges_directed(parent.node, Direction::Outgoing)
            .find(|edge| {
                self.get_game_object_from_node(edge.target())
                    .map(|go| go == sibling)
                    .unwrap_or(false)
            });
        edge.map(|edge| match dir {
            SiblingDir::Before => *edge.weight(),
            SiblingDir::After => *edge.weight() + 1,
        })
    }

    fn shift_edge_weights(&mut self, parent: GameObject, start: i32, offset: i32) {
        let mut walker = self.entity_arena.neighbors(parent.node).detach();
        while let Some((edge, _)) = walker.next(&self.entity_arena) {
            if let Some(edge_weight) = self.entity_arena.edge_weight_mut(edge) {
                if *edge_weight >= start {
                    *edge_weight += offset;
                }
            }
        }
    }

    fn swap_edge_weights(&mut self, parent: GameObject, first: i32, second: i32) {
        let find_edge = |weight: i32| {
            self.entity_arena
                .edges_directed(parent.node, Direction::Outgoing)
                .find_map(|edge| {
                    if *edge.weight() == weight {
                        Some(edge.id())
                    } else {
                        None
                    }
                })
        };
        let Some(first_edge) = find_edge(first) else {
            return;
        };
        let Some(second_edge) = find_edge(second) else {
            return;
        };
        self.entity_arena[first_edge] = second;
        self.entity_arena[second_edge] = first;
    }

    pub fn get_main_camera<'a>(
        &'a self,
        world: &'a World,
    ) -> Option<(GameObject, &'a ComponentCamera)> {
        let mut query = <(Entity, &ComponentTransform, &ComponentCamera)>::query();
        query
            .iter(world)
            .filter_map(|(e, t, c)| self.get_game_object_from_entity(*e).map(|go| (go, t, c)))
            .find(|(go, _, c)| {
                if let Some(camera) = &self.camera {
                    go == camera
                } else {
                    c.enabled
                }
            })
            .map(|(go, _, c)| (go, c))
    }

    pub(crate) fn new_game_object(&mut self, parent: Option<GameObject>) -> GameObject {
        let entity = self.world.push(());
        let node = self.entity_arena.add_node(entity);
        let game_object = GameObject { node, entity };
        self.entity_map.insert(entity, node);
        self.entity_arena.add_edge(
            parent.unwrap_or(self.root).node,
            node,
            self.next_edge_index(parent),
        );
        game_object
    }

    fn transform_cache(&self) -> RwLockReadGuard<HashMap<NodeIndex, Transform>> {
        self.transform_cache.read().unwrap()
    }

    fn transform_cache_mut(&self) -> RwLockWriteGuard<HashMap<NodeIndex, Transform>> {
        self.transform_cache.write().unwrap()
    }

    fn next_edge_index(&self, parent: Option<GameObject>) -> i32 {
        self.get_children(parent.unwrap_or(self.root)).count() as i32
    }

    pub fn bind_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) -> bool {
        self.entry_mut(game_object)
            .map(|mut e| e.add_component(component))
            .is_some()
    }

    pub fn bind_component_dyn(&mut self, game_object: GameObject, type_id: TypeId) -> bool {
        let registry = TypeRegistry::get();
        let Some(meta) = registry.trait_meta::<ReflectDefault>(type_id) else {
            return false;
        };
        let class_registry = ClassRegistry::get();
        let Some(component) = class_registry.component(type_id) else {
            return false;
        };
        let self_ptr = unsafe { self.as_ptr_mut() };
        self.entry_mut(game_object)
            .map(|mut e| {
                let result = component.bind_instance(&mut e, meta.default());
                if result {
                    if let Some(instance) = component.get_instance_mut(&mut e) {
                        instance.reset(unsafe { &mut *self_ptr }, game_object);
                    }
                }
                result
            })
            .unwrap_or(false)
    }

    pub fn get_component_ptr(
        &mut self,
        game_object: GameObject,
        component: &Box<dyn Component>,
    ) -> Option<*mut dyn Component> {
        self.entry_mut(game_object).and_then(|mut entry| {
            if let Some(instance) = component.get_instance_mut(&mut entry) {
                Some(instance as *mut dyn Component)
            } else {
                None
            }
        })
    }

    pub fn entry(&self, game_object: GameObject) -> Option<EntryRef> {
        self.world.entry_ref(game_object.entity).ok()
    }

    pub fn entry_mut(&mut self, game_object: GameObject) -> Option<Entry> {
        self.world.entry(game_object.entity)
    }

    pub fn read_component<T: Component, R, F: FnOnce(&T) -> R>(
        &self,
        game_object: GameObject,
        reader: F,
    ) -> Option<R> {
        let entry = self.entry(game_object);
        entry
            .as_ref()
            .and_then(|entry| entry.get_component::<T>().ok())
            .map(|c| reader(c))
    }

    pub fn write_component<T: Component, F: FnOnce(&mut T)>(
        &mut self,
        game_object: GameObject,
        writer: F,
    ) -> Option<()> {
        let mut entry = self.entry_mut(game_object);
        entry
            .as_mut()
            .and_then(|entry| entry.get_component_mut::<T>().ok())
            .map(|c| writer(c))
    }

    pub fn prepare(&mut self) {
        self.delete_game_objects();
        PhysicsContext::prepare(self);
    }

    pub fn update(&mut self, input: &Input) {
        let time = Time::get();
        PhysicsContext::update(self, &time, &PhysicsConfiguration::default());
        for (_, component) in ClassRegistry::get().components_update() {
            // No way around this, we want component's update method to take &mut self
            // but there's no way to do that and provide a &mut Scene
            // At worst, this is a race condition because we can guarantee that
            // this reference only lives until the end of this function
            let scene = unsafe { &mut *(self as *mut Self) };
            let game_objects = <Entity>::query()
                .iter(&self.world)
                .filter_map(|e| self.get_game_object_from_entity(*e))
                .collect::<Vec<_>>();
            for game_object in game_objects {
                if let Some(mut entry) = self.entry_mut(game_object) {
                    if let Some(instance) = component.get_instance_mut(&mut entry) {
                        instance.update(scene, game_object, input);
                    }
                }
            }
        }
    }

    fn first_update(&mut self) {}

    pub fn delete_game_objects(&mut self) {
        for game_object in self
            .objects_to_delete
            .drain()
            .collect::<Vec<_>>()
            .into_iter()
        {
            let parent = self
                .get_parent_game_object(game_object)
                .unwrap_or(self.root);
            let index = self
                .get_index_in_parent(parent, game_object, SiblingDir::Before)
                .unwrap();
            self.shift_edge_weights(parent, index, -1);
            for go in std::iter::once(game_object)
                .chain(
                    Bfs::new(&self.entity_arena, game_object.node)
                        .iter(&self.entity_arena)
                        .filter_map(|node| self.get_game_object_from_node(node)),
                )
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                self.world.remove(go.entity);
                self.entity_map.remove(&go.entity);
                self.uuid_map.remove(&self.get_game_object_uuid(go));
                self.entity_arena.remove_node(go.node);
            }
        }
    }

    pub fn game_objects<'a>(&'a self) -> impl Iterator<Item = GameObject> + 'a {
        Bfs::new(&self.entity_arena, self.root.node)
            .iter(&self.entity_arena)
            .skip(1)
            .filter_map(|n| self.get_game_object_from_node(n))
    }

    pub fn get_game_object_name(&self, game_object: GameObject) -> String {
        self.entry(game_object)
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
    }

    pub fn get_game_object_uuid(&self, game_object: GameObject) -> Uuid {
        self.entry(game_object)
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    pub fn get_game_object_by_uuid(&self, id: Uuid) -> Option<GameObject> {
        self.uuid_map.get(&id).copied()
    }

    pub fn get_parent_game_object(&self, game_object: GameObject) -> Option<GameObject> {
        self.entity_arena
            .neighbors_directed(game_object.node, Direction::Incoming)
            .next()
            .and_then(|node| self.get_game_object_from_node(node))
    }

    pub fn get_parent_uuid(&self, game_object: GameObject) -> Option<Uuid> {
        self.get_parent_game_object(game_object)
            .map(|parent| self.get_game_object_uuid(parent))
    }

    pub fn get_children<'a>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        self.entity_arena
            .neighbors(game_object.node)
            .filter_map(|node| self.get_game_object_from_node(node))
    }

    pub fn get_children_walker(&self, game_object: GameObject) -> WalkChildren {
        WalkChildren {
            walker: self.entity_arena.neighbors(game_object.node).detach(),
        }
    }

    pub fn get_children_ordered<'a>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        let mut children = self
            .entity_arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .filter_map(|edge| {
                self.get_game_object_from_node(edge.target())
                    .map(|go| (edge.weight(), go))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|c| c.0);
        children.into_iter().map(|c| c.1)
    }

    pub fn get_child_by_index(&self, game_object: GameObject, index: i32) -> Option<GameObject> {
        self.entity_arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .find(|edge| *edge.weight() == index)
            .and_then(|edge| self.get_game_object_from_node(edge.target()))
    }

    pub fn is_descendant(&self, parent: GameObject, game_object: GameObject) -> bool {
        std::iter::once(parent)
            .chain(self.get_descendants(parent))
            .find(|go| *go == game_object)
            .is_some()
    }

    pub fn get_transform(&self, game_object: GameObject) -> Transform {
        if let Some(entry) = self.entry(game_object) {
            if let Ok(c_transform) = entry.get_component::<ComponentTransform>() {
                return c_transform.transform;
            }
        }
        Transform::default()
    }

    pub fn set_transform(&mut self, game_object: GameObject, matrix: Mat4) {
        if let Some(mut entry) = self.entry_mut(game_object) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&matrix);
            }
        }
    }

    pub fn set_world_transform(&mut self, game_object: GameObject, matrix: impl Into<Mat4>) {
        let parent_transform = self
            .get_parent_game_object(game_object)
            .map_or(Mat4::identity(), |go| {
                self.get_world_transform(go).inverse_matrix
            });
        if let Some(mut entry) = self.entry_mut(game_object) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform
                    .set_local_matrix(&(parent_transform * matrix.into()));
            }
        }
    }

    pub fn get_world_transform(&self, game_object: GameObject) -> Transform {
        if let Some(transform) = self.transform_cache().get(&game_object.node) {
            return *transform;
        }
        let entry = self.entry(game_object);
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix)
            });
        if let Some(parent_node) = self.get_parent_game_object(game_object) {
            matrix = self.get_world_transform(parent_node).matrix * matrix;
        }
        let transform = matrix.into();
        self.transform_cache_mut()
            .insert(game_object.node, transform);
        transform
    }

    pub fn get_transform_relative_to(
        &self,
        game_object: GameObject,
        parent: GameObject,
    ) -> Transform {
        let transform = self.get_world_transform(game_object);
        let parent_transform = self.get_world_transform(parent);
        (parent_transform.inverse_matrix * transform.matrix).into()
    }

    pub fn clear_transform_cache(&self) {
        self.transform_cache_mut().clear();
    }

    fn map_has_component<T: Component>(&self, game_object: GameObject) -> Option<GameObject> {
        self.entry(game_object)
            .and_then(|e| e.get_component::<T>().ok().map(|_| game_object))
    }

    pub fn get_descendants<'a>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        Bfs::new(&self.entity_arena, game_object.node)
            .iter(&self.entity_arena)
            .filter_map(|node| self.get_game_object_from_node(node))
    }

    pub fn get_descendants_with_component<'a, T: Component>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        self.get_descendants(game_object)
            .filter_map(|go| self.map_has_component::<T>(go))
    }

    pub fn get_ancestors<'a>(
        &'a self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + 'a {
        let reversed_arena = Reversed(&self.entity_arena);
        Dfs::new(&reversed_arena, game_object.node)
            .iter(&self.entity_arena)
            .filter_map(|node| self.get_game_object_from_node(node))
    }

    pub fn get_ancestor_with_component<T: Component>(
        &self,
        game_object: GameObject,
    ) -> Option<GameObject> {
        self.get_ancestors(game_object)
            .find_map(|go| self.map_has_component::<T>(go))
    }
}

impl Scene {
    pub unsafe fn as_ptr_mut(&self) -> *mut Self {
        let ptr = self as *const Self;
        ptr as *mut Self
    }
}
