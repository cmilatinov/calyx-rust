use legion::Entity;
use petgraph::stable_graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::GameObject;

/// Manages the mapping between UUIDs, Entities, and NodeIndices,
/// as well as tracking pending deletions and auto-naming.
///
/// An object enters both maps together through [`GameObjectStore::insert`]
/// and leaves both through [`GameObjectStore::remove`], so a lookup by id and
/// a lookup by entity always agree.
#[derive(Default)]
pub struct GameObjectStore {
    uuid_map: HashMap<Uuid, GameObject>,
    entity_map: HashMap<Entity, (NodeIndex, Uuid)>,
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
        self.entity_map.get(&entity).map(|(node, _)| GameObject {
            node: *node,
            entity,
        })
    }

    /// Returns the UUID registered for `entity`.
    pub fn uuid(&self, entity: Entity) -> Option<Uuid> {
        self.entity_map.get(&entity).map(|(_, id)| *id)
    }

    /// Returns whether `id` is registered to an object.
    pub fn contains(&self, id: Uuid) -> bool {
        self.uuid_map.contains_key(&id)
    }

    /// Registers `game_object` under `id` and under its entity at once.
    ///
    /// The caller picks an `id` no other object holds; see
    /// [`GameObjectStore::contains`].
    pub fn insert(&mut self, id: Uuid, game_object: GameObject) {
        debug_assert!(!self.uuid_map.contains_key(&id), "id {id} is taken");
        self.uuid_map.insert(id, game_object);
        self.entity_map
            .insert(game_object.entity, (game_object.node, id));
    }

    /// Queues `game_object` for deletion.
    pub fn mark_for_deletion(&mut self, game_object: GameObject) {
        self.objects_to_delete.insert(game_object);
    }

    /// Removes both mappings of a deleted object.
    pub fn remove(&mut self, entity: Entity) {
        if let Some((_, id)) = self.entity_map.remove(&entity) {
            self.uuid_map.remove(&id);
        }
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
