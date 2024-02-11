use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use egui::Ui;
use glm::Mat4;
use indextree::{Arena, Children, NodeId};
use legion::world::{Entry, EntryRef};
use legion::{Entity, EntityStore, IntoQuery, World};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate as engine;
use crate::assets::error::AssetError;
use crate::assets::{Asset, LoadedAsset};
use crate::class_registry::ClassRegistry;
use crate::component::{Component, ComponentTransform};
use crate::component::{ComponentCamera, ComponentID};
use crate::math::Transform;
use crate::scene::Prefab;
use crate::utils::TypeUuid;

use super::error::SceneError;

#[derive(Clone, Copy)]
pub struct GameObject {
    pub node: NodeId,
    pub entity: Entity,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct SceneData {
    pub components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    pub hierarchy: HashMap<Uuid, Uuid>,
}

#[derive(Default, Debug, TypeUuid)]
#[uuid = "9946a2e7-e022-447e-8e60-528da548087f"]
pub struct Scene {
    pub world: World,
    pub entity_hierarchy: HashSet<NodeId>,
    node_map: HashMap<Entity, NodeId>,
    uuid_map: HashMap<Uuid, NodeId>,
    entity_arena: Arena<Entity>,
    transform_cache: RwLock<HashMap<NodeId, Transform>>,
    camera: Option<NodeId>,
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
            let node = scene.new_entity(None);
            let entity = scene.get_entity(node);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(component_id) {
                    if let Some(instance) = component.deserialize(data) {
                        if let Some(mut entry) = scene.world.entry(scene.get_entity(node)) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = scene.entry(entity) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                scene.uuid_map.insert(id, node);
            }
        }
        for (id, parent) in value.hierarchy {
            if let Some(entity) = scene.get_node_by_uuid(id) {
                if let Some(parent) = scene.get_node_by_uuid(parent) {
                    scene.set_parent(entity, Some(parent));
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
            if let Some(parent) = scene.get_parent_entity(scene.get_node(*entity)) {
                if let Ok(parent_id) = world
                    .entry_ref(parent)
                    .unwrap()
                    .get_component::<ComponentID>()
                {
                    hierarchy.insert(id.id, parent_id.id);
                }
            }
            for (component_id, component) in ClassRegistry::get().components_uuid() {
                let entry = world.entry_ref(*entity).unwrap();
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
        SceneData {
            hierarchy,
            components,
        }
    }
}

impl From<(&Scene, NodeId)> for SceneData {
    fn from((scene, node_id): (&Scene, NodeId)) -> Self {
        let world = &scene.world;
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();

        node_id
            .descendants(&scene.entity_arena)
            .for_each(|child_id| {
                let entity = scene.get_entity(child_id);
                let entry = world.entry_ref(entity).unwrap();
                let id = entry.get_component::<ComponentID>().unwrap();

                if let Some(parent) = scene.get_parent_entity(child_id) {
                    if let Ok(parent_id) = world
                        .entry_ref(parent)
                        .unwrap()
                        .get_component::<ComponentID>()
                    {
                        hierarchy.insert(id.id, parent_id.id);
                    }
                }

                for (component_id, component) in ClassRegistry::get().components_uuid() {
                    let entry = world.entry_ref(entity).unwrap();
                    if let Some(instance) = component.get_instance(&entry) {
                        if let Some(value) = instance.serialize() {
                            components
                                .entry(id.id)
                                .or_default()
                                .insert(*component_id, value);
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

#[allow(dead_code)]
impl Scene {
    pub fn root_entities(&self) -> &HashSet<NodeId> {
        &self.entity_hierarchy
    }

    pub fn create_entity(&mut self, id: Option<ComponentID>, parent: Option<NodeId>) -> NodeId {
        let c_id = if let Some(id_comp) = id {
            id_comp
        } else {
            ComponentID::default()
        };
        let id = c_id.id;
        let entity = self.world.push((c_id, ComponentTransform::default()));
        let new_node = self.entity_arena.new_node(entity);
        self.node_map.insert(entity, new_node);
        self.uuid_map.insert(id, new_node);

        // push into root otherwise push under parent specified
        match parent {
            None => {
                self.entity_hierarchy.insert(new_node);
            }
            Some(parent_id) => {
                parent_id.append(new_node, &mut self.entity_arena);
            }
        };

        new_node
    }

    pub fn create_prefab(&self, node_id: NodeId) -> Prefab {
        let data: SceneData = (self, node_id).into();

        Prefab {
            data: data.clone(),
            scene: data.into(),
        }
    }

    pub fn instantiate_prefab(&mut self, prefab: &Prefab, parent_opt: Option<NodeId>) {
        let root_node = prefab.scene.root_entities().iter().next().unwrap();

        for (_, components) in prefab.data.components.iter() {
            let node = self.new_entity(None);
            let entity = self.get_entity(node);
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(*component_id) {
                    if let Some(instance) = component.deserialize(data.clone()) {
                        if let Some(mut entry) = self.world.entry(entity) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
            }
            let mut id = None;
            if let Some(entry) = self.entry(entity) {
                if let Ok(c_id) = entry.get_component::<ComponentID>() {
                    id = Some(c_id.id);
                }
            }
            if let Some(id) = id {
                self.uuid_map.insert(id, node);
            }
        }

        for (id, parent) in prefab.data.hierarchy.iter() {
            if let Some(entity) = self.get_node_by_uuid(*id) {
                if let Some(parent) = self.get_node_by_uuid(*parent) {
                    self.set_parent(entity, Some(parent));
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

        if let Some(parent) = parent_opt {
            if let Some(entity) = self.get_node_by_uuid(prefab.scene.get_node_uuid(*root_node)) {
                self.set_parent(entity, Some(parent));
            }
        }
    }

    pub fn set_parent(&mut self, node: NodeId, parent: Option<NodeId>) {
        if let Some(_) = self.get_parent_node(node) {
            node.detach(&mut self.entity_arena);
        }
        if let Some(parent) = parent {
            parent.append(node, &mut self.entity_arena);
            self.entity_hierarchy.remove(&node);
        } else {
            self.entity_hierarchy.insert(node);
        }
    }

    pub fn get_main_camera<'a>(
        &'a self,
        world: &'a World,
    ) -> Option<(NodeId, &'a ComponentCamera)> {
        let mut query = <(Entity, &ComponentTransform, &ComponentCamera)>::query();
        query
            .iter(world)
            .find(|(e, _, c)| {
                if let Some(node) = &self.camera {
                    self.get_node(**e) == *node
                } else {
                    c.enabled
                }
            })
            .map(|(e, _, c)| (self.get_node(*e), c))
    }

    pub(crate) fn new_entity(&mut self, parent: Option<NodeId>) -> NodeId {
        let entity = self.world.push(());
        let node = self.entity_arena.new_node(entity);
        self.node_map.insert(entity, node);
        match parent {
            None => {
                self.entity_hierarchy.insert(node);
            }
            Some(parent_id) => {
                parent_id.append(node, &mut self.entity_arena);
            }
        };
        node
    }

    fn transform_cache(&self) -> RwLockReadGuard<HashMap<NodeId, Transform>> {
        self.transform_cache.read().unwrap()
    }

    fn transform_cache_mut(&self) -> RwLockWriteGuard<HashMap<NodeId, Transform>> {
        self.transform_cache.write().unwrap()
    }

    pub fn bind_component<T: Send + Sync + 'static>(
        &mut self,
        node_id: NodeId,
        component: T,
    ) -> Result<(), SceneError> {
        self.world
            .entry(self.get_entity(node_id))
            .map(|mut e| e.add_component(component))
            .ok_or(SceneError::InvalidNodeId)
    }

    pub fn get_component_ptr(
        &mut self,
        entity: Entity,
        component: &Box<dyn Component>,
    ) -> Option<*mut dyn Component> {
        self.world.entry(entity).and_then(|mut entry| {
            if let Some(instance) = component.get_instance_mut(&mut entry) {
                Some(instance as *mut dyn Component)
            } else {
                None
            }
        })
    }

    pub fn entry(&self, entity: Entity) -> Option<EntryRef> {
        self.world.entry_ref(entity).ok()
    }

    pub fn entry_mut(&mut self, entity: Entity) -> Option<Entry> {
        self.world.entry(entity)
    }

    pub fn update(&mut self, ui: &Ui) {
        for (_, component) in ClassRegistry::get().components_update() {
            // No way around this, we want component's update method to take &mut self
            // but there's no way to do that and provide an &mut Scene
            // At worst, this is a race condition because we can guarantee that
            // this reference only lives until the end of this function
            let scene = unsafe { &mut *(self as *mut Self) };
            let entities = <Entity>::query()
                .iter(&self.world)
                .copied()
                .collect::<Vec<_>>();
            for entity in entities {
                let node = self.get_node(entity);
                if let Some(mut entry) = self.world.entry(entity) {
                    if let Some(instance) = component.get_instance_mut(&mut entry) {
                        instance.update(scene, node, ui);
                    }
                }
            }
        }
    }

    pub fn get_entity_name(&self, node_id: NodeId) -> String {
        self.world
            .entry_ref(self.get_entity(node_id))
            .ok()
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
    }

    pub fn get_node_uuid(&self, node: NodeId) -> Uuid {
        self.world
            .entry_ref(self.get_entity(node))
            .ok()
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    pub fn get_entity_by_uuid(&self, id: Uuid) -> Option<Entity> {
        self.get_node_by_uuid(id).map(|n| self.get_entity(n))
    }

    pub fn get_node_by_uuid(&self, id: Uuid) -> Option<NodeId> {
        self.uuid_map.get(&id).copied()
    }

    pub fn get_parent_entity(&self, node_id: NodeId) -> Option<Entity> {
        let parent_node_id = self.entity_arena.get(node_id)?.parent()?;
        Some(*self.entity_arena.get(parent_node_id)?.get())
    }

    pub fn get_parent_node(&self, node_id: NodeId) -> Option<NodeId> {
        self.entity_arena.get(node_id)?.parent()
    }

    pub fn get_node(&self, entity: Entity) -> NodeId {
        *self.node_map.get(&entity).unwrap()
    }

    pub fn get_entity(&self, node_id: NodeId) -> Entity {
        *self.entity_arena.get(node_id).unwrap().get()
    }

    pub fn get_children(&self, node_id: NodeId) -> Children<'_, Entity> {
        node_id.children(&self.entity_arena)
    }

    pub fn get_children_count(&self, node_id: NodeId) -> usize {
        node_id.children(&self.entity_arena).count()
    }

    pub fn get_transform(&self, node: NodeId) -> Transform {
        let entity = self.get_entity(node);
        if let Ok(entry) = self.world.entry_ref(entity) {
            if let Ok(c_transform) = entry.get_component::<ComponentTransform>() {
                return c_transform.transform;
            }
        }
        Transform::default()
    }

    pub fn set_transform(&mut self, node: NodeId, matrix: Mat4) {
        if let Some(mut entry) = self.world.entry(self.get_entity(node)) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&matrix);
            }
        }
    }

    pub fn set_world_transform(&mut self, node_id: NodeId, matrix: Mat4) {
        let parent_transform = self.get_parent_node(node_id).map_or(Mat4::identity(), |n| {
            self.get_world_transform(n).inverse_matrix
        });
        if let Some(mut entry) = self.world.entry(self.get_entity(node_id)) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&(parent_transform * matrix));
            }
        }
    }

    pub fn get_world_transform(&self, node_id: NodeId) -> Transform {
        if let Some(transform) = self.transform_cache().get(&node_id) {
            return *transform;
        }
        let entry = self.world.entry_ref(self.get_entity(node_id));
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix)
            });
        if let Some(parent_node) = self.get_parent_node(node_id) {
            matrix = self.get_world_transform(parent_node).matrix * matrix;
        }
        let transform = matrix.into();
        self.transform_cache_mut().insert(node_id, transform);
        transform
    }

    pub fn clear_transform_cache(&self) {
        self.transform_cache_mut().clear();
    }

    pub fn get_children_with_component<'a, T: Component>(
        &'a self,
        node: NodeId,
    ) -> impl Iterator<Item = NodeId> + 'a {
        node.descendants(&self.entity_arena).filter(|c| {
            self.entry(self.get_entity(*c))
                .map(|e| e.get_component::<T>().is_ok())
                .unwrap_or(false)
        })
    }
}
