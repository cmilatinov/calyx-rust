use specs::{Builder, Component, Entity, VecStorage, World, WorldExt};
use specs::world::Index;
use crate::math::transform::Transform;
use indextree::{Arena, NodeId};
use crate::core::error::EngineError;

struct Scene {
    world: World,
    entity_hierarchy: Vec<NodeId>,
    entity_arena: Arena<Index>
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            world: World::new(),
            entity_hierarchy: Vec::new(),
            entity_arena: Arena::new()
        }
    }
}

impl Scene {
    pub fn update(&mut self) {
        self.world.maintain();
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
    /// # Example
    ///
    /// ```
    /// // Assuming `scene` is an instance of `Scene`.
    /// let node_id = /* the NodeId of the entity */;
    /// scene.bind_component_default::<MyComponent>(node_id);
    /// ```
    ///
    /// # Errors
    ///
    /// This function will return `Err(EngineError)` if the `NodeId` is invalid
    /// or if there's an error when inserting the component into the `world` storage.
    pub fn bind_component_default<T: Component<Storage=VecStorage<T>> + Sync + Send + Default>(&mut self, node_id: NodeId) -> Result<(), EngineError>{
        let entity = self.get_entity(node_id).ok_or(EngineError::new("Invalid NodeId"))?;
        self.world.write_storage().insert(entity, T::default())?;
        Ok(())
    }

    pub fn bind_component<T: Component<Storage=VecStorage<T>> + Sync + Send>(&mut self, node_id: NodeId, component: T) -> Result<(), EngineError>{
        let entity = self.get_entity(node_id).ok_or(EngineError::new("Invalid NodeId"))?;
        self.world.write_storage().insert(entity, component)?;
        Ok(())
    }

    pub fn add_entity(&mut self, parent: Option<NodeId>) -> NodeId {
        let new_entity = self.world.create_entity().with(Transform::default()).build();
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

    pub fn get_entity(&self, node_id: NodeId) -> Option<Entity> {
        let node = self.entity_arena.get(node_id)?;
        Some(self.world.entities().entity(*node.get()))
    }
}
