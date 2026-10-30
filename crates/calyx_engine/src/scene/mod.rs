//! Scene authoring and runtime APIs.
//!
//! The scene module exposes the high-level handles and managers that editor and
//! game code use to build, traverse, serialize, and simulate hierarchies of
//! game objects.

pub use self::dropped_components::{dropped_components_report, DropReason, DroppedComponent};
pub use self::error::*;
pub use self::game_object_ref::*;
pub use self::game_object_store::*;
pub use self::hud::*;
pub use self::prefab::*;
pub use self::prefab_link::{PrefabLink, PrefabUpdate};
pub use self::scene::*;
pub use self::scene_graph::{SceneGraph, SiblingDir, WalkChildren};
pub use self::scene_manager::*;
pub use self::scene_view::SceneView;
pub use self::transform_cache::*;

pub(crate) mod disabled_components;
mod dropped_components;
mod error;
pub(crate) mod game_object_ref;
mod hud;
mod prefab;
mod prefab_link;
mod scene_manager;
mod scene_view;

// These sub-modules are pub(crate) so that Scene's internal sub-systems
// (graph, store, transforms) can be accessed by sibling modules within
// the engine crate (e.g. physics, networking) while remaining hidden
// from external consumers. The public API is re-exported above.
pub(crate) mod change_set;
pub(crate) mod game_object_store;
pub(crate) mod network_index;
pub(crate) mod scene;
pub(crate) mod scene_graph;
pub(crate) mod transform_cache;

#[cfg(test)]
mod disabled_components_tests;
#[cfg(test)]
mod tests;
