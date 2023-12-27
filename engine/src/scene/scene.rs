use std::collections::HashMap;

use glm::Mat4;
use indextree::{Arena, Children, NodeId};
use legion::{Entity, EntityStore, World};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use uuid::Uuid;

use crate::assets::mesh::Mesh;
use crate::assets::AssetRegistry;
use crate::component::{ComponentCamera, ComponentID, ComponentMesh};
use crate::component::{ComponentPointLight, ComponentTransform};
use crate::math::Transform;

use super::error::SceneError;

pub struct Scene {
    pub world: RwLock<World>,
    pub entity_hierarchy: Vec<NodeId>,
    node_map: HashMap<Entity, NodeId>,
    entity_arena: Arena<Entity>,
    transform_cache: RwLock<HashMap<NodeId, Transform>>,
}

impl Default for Scene {
    fn default() -> Self {
        let mut scene = Scene {
            world: RwLock::new(World::default()),
            entity_hierarchy: Vec::new(),
            entity_arena: Arena::new(),
            node_map: HashMap::new(),
            transform_cache: RwLock::new(HashMap::new()),
        };

        let mesh = AssetRegistry::get_mut()
            .load::<Mesh>("meshes/cube")
            .unwrap();

        let cube = scene.create_entity(None, None);
        scene
            .bind_component(
                cube,
                ComponentMesh {
                    mesh: mesh.clone().into(),
                },
            )
            .unwrap();
        scene
            .bind_component(cube, ComponentPointLight::default())
            .unwrap();
        scene
            .bind_component(cube, ComponentCamera::default())
            .unwrap();

        let cube2 = scene.create_entity(
            Some(ComponentID {
                id: Uuid::new_v4(),
                name: "Bing bong".to_string(),
            }),
            None,
        );
        scene
            .bind_component(cube2, ComponentMesh { mesh: mesh.into() })
            .unwrap();

        {
            let mut world = scene.world_mut();
            let mut e_cube = world.entry(scene.get_entity(cube)).unwrap();
            e_cube
                .get_component_mut::<ComponentTransform>()
                .unwrap()
                .transform
                .translate(&glm::vec3(0.0, 0.0, 10.0));
            let mut e_cube2 = world.entry(scene.get_entity(cube2)).unwrap();
            e_cube2
                .get_component_mut::<ComponentTransform>()
                .unwrap()
                .transform
                .translate(&glm::vec3(0.0, 5.0, 0.0));
        }

        scene
    }
}

#[allow(dead_code)]
impl Scene {
    pub fn root_entities(&self) -> &Vec<NodeId> {
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
            None => self.entity_hierarchy.push(new_node),
            Some(parent_id) => parent_id.append(new_node, &mut self.entity_arena),
        };

        new_node
    }

    pub fn world(&self) -> RwLockReadGuard<World> {
        self.world.read().unwrap()
    }

    pub fn world_mut(&self) -> RwLockWriteGuard<World> {
        self.world.write().unwrap()
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

    pub fn get_entity_name(&self, node_id: NodeId) -> String {
        self.world()
            .entry_ref(self.get_entity(node_id))
            .ok()
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|c| c.name.clone()))
            .unwrap_or_default()
    }

    pub fn get_entity_uuid(&self, node_id: NodeId) -> Uuid {
        self.world()
            .entry_ref(self.get_entity(node_id))
            .ok()
            .and_then(|e| e.get_component::<ComponentID>().ok().map(|id| id.id))
            .unwrap_or_default()
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
