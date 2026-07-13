use legion::Entity;
use petgraph::stable_graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::GameObject;

/// Manages the mapping between UUIDs, Entities, and NodeIndices,
/// as well as tracking pending deletions and auto-naming.
#[derive(Default)]
pub struct GameObjectStore {
    uuid_map: HashMap<Uuid, GameObject>,
    entity_map: HashMap<Entity, NodeIndex>,
    objects_to_delete: HashSet<GameObject>,
    new_index: usize,
}

impl GameObjectStore {
    /// Resolves a game object by persistent UUID.
    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.uuid_map.get(&id).copied()
    }

    /// Resolves a game object by Legion entity.
    pub fn game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.entity_map.get(&entity).map(|node| GameObject {
            node: *node,
            entity,
        })
    }

    /// Registers both UUID and entity mappings for `game_object`.
    pub fn register(&mut self, id: Uuid, game_object: GameObject) {
        self.uuid_map.insert(id, game_object);
        self.entity_map.insert(game_object.entity, game_object.node);
    }

    /// Registers only the UUID mapping for `game_object`.
    pub fn register_uuid(&mut self, id: Uuid, game_object: GameObject) {
        self.uuid_map.insert(id, game_object);
    }

    /// Registers only the entity mapping for a scene-graph node.
    pub fn register_entity(&mut self, entity: Entity, node: NodeIndex) {
        self.entity_map.insert(entity, node);
    }

    /// Queues `game_object` for deletion.
    pub fn mark_for_deletion(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    /// Removes all mappings for a deleted object.
    pub fn remove(&mut self, uuid: Uuid, entity: Entity) {
        self.uuid_map.remove(&uuid);
        self.entity_map.remove(&entity);
    }

    /// Generates the next default game object name.
    pub fn next_name(&mut self) -> String {
        let number = if self.new_index != 0 {
            format!(" ({})", self.new_index)
        } else {
            "".into()
        };
        self.new_index += 1;
        format!("Game Object{number}")
    }

    /// Drains and returns the pending deletion queue.
    pub fn drain_deletions(&mut self) -> Vec<GameObject> {
        self.objects_to_delete.drain().collect()
    }
}
