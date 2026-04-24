use bimap::BiHashMap;
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use log::trace;
use nalgebra_glm::Mat4;
use petgraph::prelude::{EdgeRef, StableGraph};
use petgraph::stable_graph::{DefaultIx, NodeIndex, WalkNeighbors};
use petgraph::visit::{Bfs, Walker};
use petgraph::Direction;
use serde::de::DeserializeSeed;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
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
use crate::physics::{PhysicsConfiguration, PhysicsContext};
use crate::reflect::{ReflectDefault, TypeInfo};
use crate::resource::ResourceMap;
use crate::scene::{GameObjectRef, Prefab};
use crate::try_all;
use crate::utils::{ContextSeed, TypeUuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameObject {
    pub node: NodeIndex,
    pub entity: Entity,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    pub components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    pub hierarchy: HashMap<Uuid, Vec<Uuid>>,
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
    registries: ReadOnlyRegistryContext,
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
            .and_then(|node| scene.game_object_from_node(node))
    }
}

impl Scene {
    pub fn new(assets: ReadOnlyRegistryContext) -> Self {
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
        let data: SceneData = self.into();
        (&self.registries, data).into()
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
                    let instance = component.deserialize(data);
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
                scene.uuid_map.insert(id, game_object);
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
        std::iter::once(game_object.node)
            .chain(Bfs::new(&scene.entity_arena, game_object.node).iter(&scene.entity_arena))
            .filter_map(|c| scene.game_object_from_node(c))
            .for_each(|game_object| {
                let Some(entry) = scene.entry(game_object) else {
                    return;
                };
                let Ok(id) = entry.get_component::<ComponentID>() else {
                    return;
                };
                scene.serialize_game_object(game_object, id.id, &mut data);
            });
        data
    }
}

impl Scene {
    pub(crate) fn game_object_from_node(&self, node: NodeIndex) -> Option<GameObject> {
        self.entity_arena
            .node_weight(node)
            .map(|e| GameObject { node, entity: *e })
    }

    pub(crate) fn game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.entity_map.get(&entity).map(|node| GameObject {
            node: *node,
            entity,
        })
    }

    pub(crate) fn serialize_game_object(
        &self,
        game_object: GameObject,
        game_object_id: Uuid,
        data: &mut SceneData,
    ) {
        'insert_hierarchy: {
            try_all!(
                None => break 'insert_hierarchy;
                let parent = self.parent(game_object);
                let entry = self.entry(parent);
                let parent_id = entry.get_component::<ComponentID>().ok();
            );
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
    pub fn root(&self) -> GameObject {
        self.root
    }

    pub fn root_id(&self) -> Uuid {
        self.uuid(self.root)
    }

    pub fn root_objects(&self) -> impl Iterator<Item = GameObject> + '_ {
        self.children_ordered(self.root)
    }

    pub fn prefab_root(&self) -> Option<GameObject> {
        self.root_objects().next()
    }

    pub fn create(&mut self, id: Option<ComponentID>, parent: Option<GameObject>) -> GameObject {
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

    pub fn delete(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    pub fn create_prefab(&self, game_object: GameObject) -> Prefab {
        let data: SceneData = (self, game_object).into();

        Prefab {
            data: data.clone(),
            scene: (&self.registries, data).into(),
        }
    }

    pub fn instantiate_prefab(
        &mut self,
        prefab: &Prefab,
        parent: Option<GameObject>,
    ) -> Option<GameObject> {
        let root_node = prefab.scene.prefab_root().unwrap();

        let mut id_mapping = prefab
            .data
            .components
            .iter()
            .map(|(game_object_id, id)| (*game_object_id, Uuid::new_v4()))
            .collect::<BiHashMap<_, _>>();

        let asset_registry_ref = self.registries.assets.clone();
        let asset_registry = asset_registry_ref.read();
        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let type_registry_ref = self.registries.types.clone();
        let type_registry = type_registry_ref.read();
        for (game_object_id, components) in prefab.data.components.iter() {
            let game_object = self.new_game_object(None);
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).unwrap();
            self.uuid_map.insert(new_game_object_id, game_object);
        }

        for (game_object_id, components) in prefab.data.components.iter() {
            let new_game_object_id = *id_mapping.get_by_left(game_object_id).unwrap();
            let game_object = self.find(new_game_object_id).unwrap();
            for (component_id, data) in components {
                try_all!(
                    None => continue;
                    let TypeInfo::Struct(struct_info) = type_registry.type_info_by_id(*component_id);
                    let component = component_registry.component(*component_id);
                    let mut instance = component.deserialize(data.clone());
                    let mut entry = self.entry_mut(game_object);
                );
                for (name, field) in &struct_info.fields {
                    try_all!(
                        None => continue;
                        let id = field.get::<GameObjectRef>(&*instance).map(|r| r.id());
                        let target_id = id_mapping.get_by_left(&id);
                    );
                    trace!("{} - {}", name, field.name);
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
            let prefab_uuid = id_mapping.get_by_left(&prefab.scene.uuid(root_node));
            let game_object = self.find(*prefab_uuid);
        );

        if let Some(parent) = parent {
            self.set_parent(game_object, Some(parent));
        }

        Some(game_object)
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
            if let Some(index) = self.index_in_parent(parent, sibling, dir) {
                if let Some(current) = self.index_in_parent(parent, game_object, SiblingDir::Before)
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

        if let Some((parent, edge)) = self.parent(game_object).and_then(|parent| {
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

    pub fn index_in_parent(
        &self,
        parent: GameObject,
        sibling: GameObject,
        dir: SiblingDir,
    ) -> Option<i32> {
        let edge = self
            .entity_arena
            .edges_directed(parent.node, Direction::Outgoing)
            .find(|edge| {
                self.game_object_from_node(edge.target())
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

    pub fn main_camera(&self) -> Option<(GameObject, &ComponentCamera)> {
        let mut query = <(Entity, &ComponentTransform, &ComponentCamera)>::query();
        query
            .iter(&self.world)
            .filter_map(|(e, t, c)| self.game_object_from_entity(*e).map(|go| (go, c)))
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

    fn transform_cache(&self) -> RwLockReadGuard<'_, HashMap<NodeIndex, Transform>> {
        self.transform_cache.read().unwrap()
    }

    fn transform_cache_mut(&self) -> RwLockWriteGuard<'_, HashMap<NodeIndex, Transform>> {
        self.transform_cache.write().unwrap()
    }

    fn next_edge_index(&self, parent: Option<GameObject>) -> i32 {
        self.children(parent.unwrap_or(self.root)).count() as i32
    }

    pub fn add_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) {
        let component_uuid = component.uuid();
        self.bind_component(game_object, component);
    }

    pub(crate) fn bind_component<T: Component + Send + Sync + 'static>(
        &mut self,
        game_object: GameObject,
        component: T,
    ) {
        self.entry_mut(game_object)
            .map(|mut e| e.add_component(component));
    }

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
        let assets = self.registries.clone();
        let default_instance = meta.default();
        let Some(mut entry) = self.entry_mut(game_object) else {
            return;
        };
        let result = component.bind_instance(&mut entry, default_instance);
        if result {
            // Take the component out so we can call reset() with &mut Scene safely.
            if let Some(mut instance) = component.take_instance(&mut entry) {
                drop(entry);
                instance.reset(ComponentEventContext {
                    registries: &assets,
                    scene: self,
                    game_object,
                });
                // Put it back.
                if let Some(mut entry) = self.entry_mut(game_object) {
                    component.put_back_instance(&mut entry, instance);
                }
            }
        }
    }

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

    pub fn entry(&self, game_object: GameObject) -> Option<EntryRef<'_>> {
        self.world.entry_ref(game_object.entity).ok()
    }

    pub fn entry_mut(&mut self, game_object: GameObject) -> Option<Entry<'_>> {
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
            .map(reader)
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
            .map(writer)
    }

    pub fn prepare(&mut self) {
        self.flush_deletes();
        self.clear_transform_cache();
        PhysicsContext::prepare(self);
    }

    pub fn update(&mut self, resources: &mut ResourceMap, input: &Input) {
        PhysicsContext::update(self, resources.time(), &PhysicsConfiguration::default());
        let component_registry_ref = self.registries.components.clone();
        let component_registry = component_registry_ref.read();
        let assets = self.registries.clone();

        for (_, component) in component_registry.components_update() {
            let game_objects: Vec<GameObject> = <Entity>::query()
                .iter(&self.world)
                .filter_map(|e| self.game_object_from_entity(*e))
                .collect();

            for game_object in game_objects {
                // Take the component out of the World so we hold an owned value.
                // This eliminates the aliased &mut self UB: the component is no
                // longer inside the Scene while we pass &mut Scene to update().
                let Some(mut entry) = self.entry_mut(game_object) else {
                    continue;
                };
                let Some(mut instance) = component.take_instance(&mut entry) else {
                    continue;
                };
                drop(entry);

                instance.update(
                    ComponentEventContext {
                        registries: &assets,
                        scene: self,
                        game_object,
                    },
                    resources,
                    input,
                );

                // Put the component back into the World.
                let Some(mut entry) = self.entry_mut(game_object) else {
                    continue;
                };
                component.put_back_instance(&mut entry, instance);
            }
        }
    }

    pub fn flush_deletes(&mut self) {
        for game_object in self
            .objects_to_delete
            .drain()
            .collect::<Vec<_>>()
            .into_iter()
        {
            let parent = self.parent(game_object).unwrap_or(self.root);
            let index = self
                .index_in_parent(parent, game_object, SiblingDir::Before)
                .unwrap();
            self.shift_edge_weights(parent, index, -1);
            for go in std::iter::once(game_object)
                .chain(
                    Bfs::new(&self.entity_arena, game_object.node)
                        .iter(&self.entity_arena)
                        .filter_map(|node| self.game_object_from_node(node)),
                )
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let uuid = self.uuid(go);
                self.world.remove(go.entity);
                self.entity_map.remove(&go.entity);
                self.uuid_map.remove(&uuid);
                self.entity_arena.remove_node(go.node);
            }
        }
    }

    pub fn objects(&self) -> impl Iterator<Item = GameObject> + '_ {
        Bfs::new(&self.entity_arena, self.root.node)
            .iter(&self.entity_arena)
            .skip(1)
            .filter_map(|n| self.game_object_from_node(n))
    }

    pub fn name(&self, game_object: GameObject) -> String {
        self.entry(game_object)
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
    }

    pub fn uuid(&self, game_object: GameObject) -> Uuid {
        self.entry(game_object)
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.uuid_map.get(&id).copied()
    }

    pub fn parent(&self, game_object: GameObject) -> Option<GameObject> {
        self.entity_arena
            .neighbors_directed(game_object.node, Direction::Incoming)
            .next()
            .and_then(|node| self.game_object_from_node(node))
    }

    pub fn parent_uuid(&self, game_object: GameObject) -> Option<Uuid> {
        self.parent(game_object).map(|parent| self.uuid(parent))
    }

    pub fn children(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        self.entity_arena
            .neighbors(game_object.node)
            .filter_map(|node| self.game_object_from_node(node))
    }

    pub fn children_walker(&self, game_object: GameObject) -> WalkChildren {
        WalkChildren {
            walker: self.entity_arena.neighbors(game_object.node).detach(),
        }
    }

    pub fn children_ordered(
        &self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + '_ {
        let mut children = self
            .entity_arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .filter_map(|edge| {
                self.game_object_from_node(edge.target())
                    .map(|go| (edge.weight(), go))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|c| c.0);
        children.into_iter().map(|c| c.1)
    }

    pub fn child_at(&self, game_object: GameObject, index: i32) -> Option<GameObject> {
        self.entity_arena
            .edges_directed(game_object.node, Direction::Outgoing)
            .find(|edge| *edge.weight() == index)
            .and_then(|edge| self.game_object_from_node(edge.target()))
    }

    pub fn is_descendant(&self, parent: GameObject, game_object: GameObject) -> bool {
        std::iter::once(parent)
            .chain(self.descendants(parent))
            .any(|go| go == game_object)
    }

    pub fn transform(&self, game_object: GameObject) -> Transform {
        let Some(entry) = self.entry(game_object) else {
            return Default::default();
        };
        let Ok(c_transform) = entry.get_component::<ComponentTransform>() else {
            return Default::default();
        };
        c_transform.transform
    }

    pub fn set_transform(&mut self, game_object: GameObject, matrix: &Mat4) {
        let Some(mut entry) = self.entry_mut(game_object) else {
            return;
        };
        let Ok(c_transform) = entry.get_component_mut::<ComponentTransform>() else {
            return;
        };
        c_transform.transform.set_local_matrix(matrix);
    }

    pub fn set_world_transform(&mut self, game_object: GameObject, matrix: impl Into<Mat4>) {
        let parent_transform = self.parent(game_object).map_or(Mat4::identity(), |go| {
            self.world_transform(go).inverse_matrix()
        });
        self.set_transform(game_object, &(parent_transform * matrix.into()));
    }

    pub fn world_transform(&self, game_object: GameObject) -> Transform {
        if let Some(transform) = self.transform_cache().get(&game_object.node) {
            return *transform;
        }
        let entry = self.entry(game_object);
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix())
            });
        if let Some(parent_node) = self.parent(game_object) {
            if parent_node != game_object {
                matrix = self.world_transform(parent_node).matrix() * matrix;
            }
        }
        let transform = matrix.into();
        self.transform_cache_mut()
            .insert(game_object.node, transform);
        transform
    }

    pub fn transform_relative_to(&self, game_object: GameObject, parent: GameObject) -> Transform {
        let transform = self.world_transform(game_object);
        let parent_transform = self.world_transform(parent);
        (parent_transform.inverse_matrix() * transform.matrix()).into()
    }

    pub fn clear_transform_cache(&self) {
        self.transform_cache_mut().clear();
    }

    fn map_has_component<T: Component>(&self, game_object: GameObject) -> Option<GameObject> {
        self.entry(game_object)
            .and_then(|e| e.get_component::<T>().ok().map(|_| game_object))
    }

    pub fn descendants(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        Bfs::new(&self.entity_arena, game_object.node)
            .iter(&self.entity_arena)
            .filter_map(|node| self.game_object_from_node(node))
    }

    pub fn descendants_with<T: Component>(
        &self,
        game_object: GameObject,
    ) -> impl Iterator<Item = GameObject> + '_ {
        self.descendants(game_object)
            .filter_map(|go| self.map_has_component::<T>(go))
    }

    pub fn ancestors(&self, game_object: GameObject) -> impl Iterator<Item = GameObject> + '_ {
        std::iter::successors(self.parent(game_object), |go| self.parent(*go))
    }

    pub fn ancestor_with<T: Component>(&self, game_object: GameObject) -> Option<GameObject> {
        self.ancestors(game_object)
            .find_map(|go| self.map_has_component::<T>(go))
    }

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
