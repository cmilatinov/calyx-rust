use nalgebra_glm::Mat4;
use petgraph::stable_graph::NodeIndex;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::component::ComponentTransform;
use crate::math::Transform;
use legion::EntityStore;

use super::scene_graph::SceneGraph;
use super::GameObject;

/// Caches world-space transforms to avoid redundant recomputation each frame.
/// The cache is cleared at the start of each frame via `clear()`.
pub struct TransformCache {
    cache: RwLock<HashMap<NodeIndex, Transform>>,
}

impl Default for TransformCache {
    fn default() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl TransformCache {
    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }

    pub fn get(&self, node: NodeIndex) -> Option<Transform> {
        self.cache.read().unwrap().get(&node).copied()
    }

    pub fn insert(&self, node: NodeIndex, transform: Transform) {
        self.cache.write().unwrap().insert(node, transform);
    }

    /// Compute the world transform for a game object by walking up the hierarchy.
    /// Results are cached for subsequent lookups within the same frame.
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
