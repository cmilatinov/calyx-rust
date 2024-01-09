use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use egui::Ui;
use glm::Mat4;
use indextree::{Arena, Children, NodeId};
use legion::{Entity, EntityStore, IntoQuery, World};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::class_registry::ClassRegistry;
use crate::component::ComponentID;
use crate::component::ComponentTransform;
use crate::math::Transform;

use super::error::SceneError;

#[derive(Serialize, Deserialize)]
pub struct SceneData {
    components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>>,
    hierarchy: HashMap<Uuid, Uuid>,
}

#[derive(Default, Debug)]
pub struct Scene {
    pub world: RwLock<World>,
    pub entity_hierarchy: HashSet<NodeId>,
    node_map: HashMap<Entity, NodeId>,
    entity_arena: Arena<Entity>,
    transform_cache: RwLock<HashMap<NodeId, Transform>>,
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
            for (component_id, data) in components {
                if let Some(component) = ClassRegistry::get().component_by_uuid(component_id) {
                    if let Some(instance) = component.deserialize(data) {
                        if let Some(mut entry) = scene.world_mut().entry(scene.get_entity(node)) {
                            let _ = component.bind_instance(&mut entry, instance);
                        }
                    }
                }
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
        let world = scene.world();
        let mut query = <(Entity, &ComponentID)>::query();
        let mut hierarchy = HashMap::new();
        let mut components: HashMap<Uuid, HashMap<Uuid, serde_json::Value>> = HashMap::new();
        for (entity, id) in query.iter(world.deref()) {
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

#[allow(dead_code)]
impl Scene {
    pub fn root_entities(&self) -> &HashSet<NodeId> {
        &self.entity_hierarchy
    }

    pub fn create_entity(&mut self, id: Option<ComponentID>, parent: Option<NodeId>) -> NodeId {
        let id = if let Some(id_comp) = id {
            id_comp
        } else {
            ComponentID::default()
        };
        let entity = self.world_mut().push((id, ComponentTransform::default()));
        let new_node = self.entity_arena.new_node(entity);
        self.node_map.insert(entity, new_node);

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

    pub fn world(&self) -> RwLockReadGuard<World> {
        self.world.read().unwrap()
    }

    pub fn world_mut(&self) -> RwLockWriteGuard<World> {
        self.world.write().unwrap()
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

    fn new_entity(&mut self, parent: Option<NodeId>) -> NodeId {
        let entity = self.world_mut().push(());
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
        self.world_mut()
            .entry(self.get_entity(node_id))
            .map(|mut e| e.add_component(component))
            .ok_or(SceneError::InvalidNodeId)
    }

    pub fn update(&self, ui: &mut Ui) {
        for node_id in self.root_entities() {
            self._update(ui, node_id.clone());
        }
    }

    pub fn _update(&self, ui: &mut Ui, node_id: NodeId) {
        // Call update on every component
        {
            let entity = self.get_entity(node_id);
            let mut world = self.world_mut();
            for (_, component) in ClassRegistry::get().components() {
                if let Some(mut entry) = world.entry(entity) {
                    if let Some(instance) = component.get_instance_mut(&mut entry) {
                        instance.update(self);
                    }
                }
            }
        }

        // Recursive call for all children nodes
        for child_id in self.get_children(node_id) {
            self._update(ui, child_id);
        }
    }

    pub fn get_entity_name(&self, node_id: NodeId) -> String {
        self.world()
            .entry_ref(self.get_entity(node_id))
            .ok()
            .and_then(|e| {
                e.get_component::<ComponentID>()
                    .ok()
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default()
    }

    pub fn get_entity_uuid(&self, node_id: NodeId) -> Uuid {
        self.world()
            .entry_ref(self.get_entity(node_id))
            .ok()
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
    }

    pub fn get_entity_by_uuid(&self, id: Uuid) -> Option<Entity> {
        let world = self.world();
        <(Entity, &ComponentID)>::query()
            .iter(world.deref())
            .find(|(_, cid)| cid.id == id)
            .map(|(entity, _)| *entity)
    }

    pub fn get_node_by_uuid(&self, id: Uuid) -> Option<NodeId> {
        self.get_entity_by_uuid(id)
            .map(|entity| self.get_node(entity))
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

    pub fn set_world_transform(&self, node_id: NodeId, matrix: Mat4) {
        let parent_transform = self.get_parent_node(node_id).map_or(Mat4::identity(), |n| {
            self.get_world_transform(n).inverse_matrix
        });
        let mut world = self.world_mut();
        if let Some(mut entry) = world.entry(self.get_entity(node_id)) {
            if let Ok(tc) = entry.get_component_mut::<ComponentTransform>() {
                tc.transform.set_local_matrix(&(parent_transform * matrix));
            }
        }
    }

    pub fn get_world_transform(&self, node_id: NodeId) -> Transform {
        if let Some(transform) = self.transform_cache().get(&node_id) {
            return *transform;
        }
        let world = self.world();
        let entry = world.entry_ref(self.get_entity(node_id));
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix)
            });
        if let Some(parent_node) = self.get_parent_node(node_id) {
            matrix = self.get_world_transform(parent_node).matrix * matrix;
        }
        let transform = Transform::from_matrix(matrix);
        self.transform_cache_mut().insert(node_id, transform);
        transform
    }

    pub fn clear_transform_cache(&mut self) {
        self.transform_cache_mut().clear();
    }
}
