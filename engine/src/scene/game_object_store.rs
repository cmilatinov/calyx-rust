use legion::Entity;
use petgraph::stable_graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::GameObject;

/// Manages the mapping between UUIDs, Entities, and NodeIndices,
/// as well as tracking pending deletions and auto-naming.
pub struct GameObjectStore {
    uuid_map: HashMap<Uuid, GameObject>,
    entity_map: HashMap<Entity, NodeIndex>,
    objects_to_delete: HashSet<GameObject>,
    new_index: usize,
}

impl Default for GameObjectStore {
    fn default() -> Self {
        Self {
            uuid_map: HashMap::new(),
            entity_map: HashMap::new(),
            objects_to_delete: HashSet::new(),
            new_index: 0,
        }
    }
}

impl GameObjectStore {
    pub fn find(&self, id: Uuid) -> Option<GameObject> {
        self.uuid_map.get(&id).copied()
    }

    pub fn game_object_from_entity(&self, entity: Entity) -> Option<GameObject> {
        self.entity_map.get(&entity).map(|node| GameObject {
            node: *node,
            entity,
        })
    }

    pub fn register(&mut self, id: Uuid, game_object: GameObject) {
        self.uuid_map.insert(id, game_object);
        self.entity_map.insert(game_object.entity, game_object.node);
    }

    pub fn register_uuid(&mut self, id: Uuid, game_object: GameObject) {
        self.uuid_map.insert(id, game_object);
    }

    pub fn register_entity(&mut self, entity: Entity, node: NodeIndex) {
        self.entity_map.insert(entity, node);
    }

    pub fn mark_for_deletion(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    pub fn remove(&mut self, uuid: Uuid, entity: Entity) {
        self.uuid_map.remove(&uuid);
        self.entity_map.remove(&entity);
    }

    pub fn next_name(&mut self) -> String {
        let number = if self.new_index != 0 {
            format!(" ({})", self.new_index)
        } else {
            "".into()
        };
        self.new_index += 1;
        format!("Game Object{}", number)
    }

    pub fn drain_deletions(&mut self) -> Vec<GameObject> {
        self.objects_to_delete.drain().collect()
    }
}
