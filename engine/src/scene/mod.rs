//! Scene authoring and runtime APIs.
//!
//! The scene module exposes the high-level handles and managers that editor and
//! game code use to build, traverse, serialize, and simulate hierarchies of
//! game objects.

pub use self::error::*;
pub use self::game_object_ref::*;
pub use self::game_object_store::*;
pub use self::prefab::*;
pub use self::scene::*;
pub use self::scene_graph::{SceneGraph, SiblingDir, WalkChildren};
pub use self::scene_manager::*;
pub use self::transform_cache::*;

mod error;
mod game_object_ref;
mod prefab;
mod scene_manager;

// These sub-modules are pub(crate) so that Scene's internal sub-systems
// (graph, store, transforms) can be accessed by sibling modules within
// the engine crate (e.g. physics, networking) while remaining hidden
// from external consumers. The public API is re-exported above.
pub(crate) mod game_object_store;
pub(crate) mod scene;
pub(crate) mod scene_graph;
pub(crate) mod transform_cache;

#[cfg(test)]
mod tests;
