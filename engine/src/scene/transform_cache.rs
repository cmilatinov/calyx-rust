use nalgebra_glm::Mat4;
use petgraph::stable_graph::NodeIndex;
use std::sync::Mutex;

use crate::component::ComponentTransform;
use crate::math::Transform;
use legion::EntityStore;

use super::scene_graph::SceneGraph;
use super::GameObject;

#[derive(Clone, Copy)]
struct TransformCacheEntry {
    transform: Transform,
    dirty: bool,
}

impl Default for TransformCacheEntry {
    fn default() -> Self {
        Self {
            transform: Default::default(),
            dirty: true,
        }
    }
}

/// Caches world-space transforms by scene graph node.
///
/// Entries stay cached until the owning node, or one of its ancestors, changes.
/// Scene mutation paths mark affected subtrees dirty so static objects can keep
/// their cached world transform across frames.
#[derive(Default)]
pub struct TransformCache {
    entries: Mutex<Vec<Option<TransformCacheEntry>>>,
}

impl TransformCache {
    pub fn clear(&self) {
        for entry in self.entries.lock().unwrap().iter_mut().flatten() {
            entry.dirty = true;
        }
    }

    pub fn mark_dirty(&self, node: NodeIndex) {
        let mut entries = self.entries.lock().unwrap();
        Self::entry_mut(&mut entries, node).dirty = true;
    }

    pub fn mark_dirty_subtree(&self, game_object: GameObject, graph: &SceneGraph) {
        for node in graph.descendant_nodes(game_object.node) {
            self.mark_dirty(node);
        }
    }

    fn get(&self, node: NodeIndex) -> Option<Transform> {
        self.entries
            .lock()
            .unwrap()
            .get(node.index())
            .and_then(|entry| entry.as_ref())
            .and_then(|entry| (!entry.dirty).then_some(entry.transform))
    }

    fn insert(&self, node: NodeIndex, transform: Transform) {
        let mut entries = self.entries.lock().unwrap();
        let entry = Self::entry_mut(&mut entries, node);
        entry.transform = transform;
        entry.dirty = false;
    }

    fn entry_mut(
        entries: &mut Vec<Option<TransformCacheEntry>>,
        node: NodeIndex,
    ) -> &mut TransformCacheEntry {
        let index = node.index();
        if entries.len() <= index {
            entries.resize(index + 1, None);
        }
        entries[index].get_or_insert_with(TransformCacheEntry::default)
    }

    /// Compute the world transform for a game object by walking up the hierarchy.
    /// Cached results are reused until dirty propagation invalidates them.
    pub fn world_transform(
        &self,
        game_object: GameObject,
        world: &legion::World,
        graph: &SceneGraph,
    ) -> Transform {
        if let Some(transform) = self.get(game_object.node) {
            return transform;
        }
        let entry = world.entry_ref(game_object.entity).ok();
        let mut matrix = entry
            .as_ref()
            .map(|e| e.get_component::<ComponentTransform>().ok())
            .map_or(Mat4::identity(), |co| {
                co.map_or(Mat4::identity(), |c| c.transform.matrix())
            });
        if let Some(parent_node) = graph.parent(game_object) {
            if parent_node != game_object {
                matrix = self.world_transform(parent_node, world, graph).matrix() * matrix;
            }
        }
        let transform = matrix.into();
        self.insert(game_object.node, transform);
        transform
    }
}
