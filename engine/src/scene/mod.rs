use specs::{Builder, Component, Entity, VecStorage, World, WorldExt};
use specs::world::Index;
use crate::math::transform::Transform;
use indextree::{Arena, NodeId};

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

pub enum SceneError {
    InvalidNodeId
}

impl Scene {
    pub fn update(&mut self) {
        self.world.maintain();
    }

    pub fn bind_component_default<T: Component<Storage=VecStorage<T>> + Sync + Send + Default>(&mut self, node_id: NodeId) -> Result<(), SceneError>{
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        self.world.write_storage().insert(entity, T::default()).expect("TODO: implement trait `From<specs::error::Error>` is not implemented for `SceneError`");
        Ok(())
    }

    pub fn bind_component<T: Component<Storage=VecStorage<T>> + Sync + Send>(&mut self, node_id: NodeId, component: T) -> Result<(), SceneError>{
        let entity = self.get_entity(node_id).ok_or(SceneError::InvalidNodeId)?;
        self.world.write_storage().insert(entity, component).expect("TODO: implement trait `From<specs::error::Error>` is not implemented for `SceneError`");
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
