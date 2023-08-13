use glm::Mat4;
use indextree::{Arena, NodeId, Node, Children};
use specs::{Builder, Entity, VecStorage, World, WorldExt};
use specs::world::Index;
use uuid::Uuid;

use error::SceneError;

use crate::assets::{AssetRegistry};
use crate::assets::mesh::Mesh;
use crate::ecs::{ComponentID, ComponentMesh};
use crate::ecs::ComponentTransform;

pub mod error;

pub struct Scene {
    pub world: World,
    pub entity_hierarchy: Vec<NodeId>,
    entity_arena: Arena<Index>
}

impl Default for Scene {
    fn default() -> Self {
        let mut world = World::new();
        world.register::<ComponentID>();
        world.register::<ComponentTransform>();
        world.register::<ComponentMesh>();
        let mut scene = Scene {
            world,
            entity_hierarchy: Vec::new(),
            entity_arena: Arena::new()
        };

        let mesh = AssetRegistry::get_mut().load::<Mesh>("meshes/cube").unwrap();

        let cube = scene.create_entity(None, None);
        scene.bind_component(cube, ComponentMesh { mesh: mesh.clone() }).unwrap();

        let cube2 = scene.create_entity(Some(ComponentID {
            id: Uuid::new_v4(),
            name: "Bing bong".to_string()
        }), Some(cube));
        scene.bind_component(cube2, ComponentMesh { mesh: mesh.clone() }).unwrap();

        {
            let mut t_s = scene.world.write_storage::<ComponentTransform>();
            t_s.get_mut(scene.get_entity(cube).unwrap())
                .unwrap().transform.translate(&glm::vec3(0.0, 0.0, 0.0));
            t_s.get_mut(scene.get_entity(cube2).unwrap())
                .unwrap().transform.translate(&glm::vec3(0.0, 5.0, 0.0));
        }

        scene
    }
}

#[allow(dead_code)]
impl Scene {
    pub fn update(&mut self) {
        self.world.maintain();
    }

    pub fn root_entities(&self) -> &Vec<NodeId> {
        &self.entity_hierarchy
    }

    /// Binds a component of type `T` to an entity in the ECS, given by its `NodeId`.
    ///
    /// The function will create a default instance of the component `T` and associate it with the entity.
    /// The component `T` must implement the `Component` trait with `VecStorage`, as well as `Sync`, `Send`, and `Default` traits.
    ///
    /// # Arguments
    ///
    /// * `node_id` - The `NodeId` of the entity to which the component should be bound.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the component was successfully bound to the entity.
    /// * `Err(EngineError)` if the entity could not be found for the given `NodeId`,
    ///   or if an error occurred while writing the component to storage in the ECS world.
    ///
    /// # Errors
    ///
    /// This function will return `Err(EngineError)` if the `NodeId` is invalid
    /// or if there's an error when inserting the component into the `world` storage.
    pub fn bind_component_default<T: specs::Component<Storage=VecStorage<T>> + Sync + Send + Default>(
        &mut self, node_id: NodeId
    ) -> Result<(), SceneError>{
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        self.world.write_storage().insert(entity, T::default())?;
        Ok(())
    }

    pub fn bind_component<T: specs::Component<Storage=VecStorage<T>> + Sync + Send>(
        &mut self, node_id: NodeId, component: T
    ) -> Result<(), SceneError>{
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        self.world.write_storage().insert(entity, component)?;
        Ok(())
    }

    pub fn create_entity(&mut self, id: Option<ComponentID>, parent: Option<NodeId>) -> NodeId {
        let new_entity = self.world.create_entity()
            .with(
                if let Some(id_comp) = id { id_comp }
                else { ComponentID::new() }
            )
            .with(ComponentTransform::default())
            .build();
        let new_node = self.entity_arena.new_node(new_entity.id());

        // push into root otherwise push under parent specified
        match parent {
            None => {
                self.entity_hierarchy.push(new_node);
            }
            Some(parent_id) => {
                parent_id.append(new_node, &mut self.entity_arena);
            }
        }

        new_node
    }

    pub fn get_parent_entity(&self, node_id: NodeId) -> Option<Entity> {
        let parent_node_id = self.entity_arena.get(node_id)?.parent()?;
        let parent_node = self.entity_arena.get(parent_node_id)?;
        Some(self.world.entities().entity(*parent_node.get()))
    }

    pub fn get_parent_node(&self, node_id: NodeId) -> Option<NodeId> {
        self.entity_arena.get(node_id)?.parent()
    }

    pub fn get_node(&self, node_id: NodeId) -> Option<&Node<Index>> {
        self.entity_arena.get(node_id)
    }

    pub fn get_entity(&self, node_id: NodeId) -> Option<Entity> {
        let node = self.entity_arena.get(node_id)?;
        Some(self.world.entities().entity(*node.get()))
    }

    pub fn get_children(&self, node_id: NodeId) -> Children<'_, Index> {
        node_id.children(&self.entity_arena)
    }

    pub fn get_children_count(&self, node_id: NodeId) -> usize {
        node_id.children(&self.entity_arena).into_iter().count()
    }

    pub fn get_entity_matrix(&self, node_id: NodeId) -> Result<Mat4, SceneError> {
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        let storage = self.world.read_storage::<ComponentTransform>();
        let entity_transform = &storage.get(entity).ok_or(SceneError::ComponentNotBound)?.transform;

        return match self.get_parent_entity(node_id) {
            None => {
                Ok(entity_transform.get_matrix())
            }
            Some(parent) => {
                let parent_transform = &storage.get(parent).ok_or(SceneError::ComponentNotBound)?.transform;
                Ok(parent_transform.get_matrix() * entity_transform.get_matrix())
            }
        }
    }

    pub fn get_entity_inverse_matrix(&self, node_id: NodeId) -> Result<Mat4, SceneError> {
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        let storage = self.world.read_storage::<ComponentTransform>();
        let entity_transform = &storage.get(entity).ok_or(SceneError::ComponentNotBound)?.transform;

        return match self.get_parent_entity(node_id) {
            None => {
                Ok(glm::inverse(&entity_transform.get_matrix()))
            }
            Some(_) => {
                let parent_inverse_matrix = self.get_entity_inverse_matrix(self.get_parent_node(node_id).ok_or(SceneError::InvalidNodeId)?)?;
                Ok(glm::inverse(&(entity_transform.get_matrix() * parent_inverse_matrix)))
            }
        }
    }

    pub fn set_entity_world_matrix(&self, node_id: NodeId, matrix: &Mat4) -> Result<(), SceneError> {
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        let mut storage = self.world.write_storage::<ComponentTransform>();
        let transform = &mut storage.get_mut(entity).ok_or(SceneError::ComponentNotBound)?.transform;

        match self.get_parent_entity(node_id) {
            None => {
                transform.set_local_matrix(matrix);
            }
            Some(parent) => {
                let mut parent_storage = self.world.write_storage::<ComponentTransform>();
                let parent_transform = &mut parent_storage.get_mut(parent).ok_or(SceneError::ComponentNotBound)?.transform;
                transform.set_local_matrix(&(parent_transform.get_matrix() * transform.get_matrix()));
            }
        }

        Ok(())
    }
}
