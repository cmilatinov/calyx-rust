//! Core engine runtime, asset pipeline, editor-facing reflection, and support
//! systems shared across the Calyx workspace.

#![allow(clippy::module_inception)]
extern crate self as engine;

/// Asset types, asset handles, and asset registry infrastructure.
pub mod assets;
/// Background task scheduling utilities.
pub mod background;
/// Reflected component registry and lookup APIs.
pub mod class_registry;
/// Component traits and built-in component types.
pub mod component;
/// Shared registry, asset, and game runtime contexts.
pub mod context;
/// Shared smart-pointer and time utilities.
pub mod core;
/// Common engine error aliases.
pub mod error;
/// Extension traits for third-party types used by the engine.
pub mod ext;
/// Input sampling, action maps, and binding helpers.
pub mod input;
/// Logging setup and logger implementations.
pub mod logging;
/// Public macros re-exported by the engine crate.
pub mod macros;
/// Transform math and other math helpers.
pub mod math;
/// Multiplayer networking types and synchronization systems.
pub mod net;
/// Physics configuration, state, and debug helpers.
pub mod physics;
/// Runtime reflection APIs and type metadata.
pub mod reflect;
/// Rendering contexts, shaders, and scene rendering APIs.
pub mod render;
/// Runtime resource storage and access.
pub mod resource;
/// Scene graphs, game objects, prefabs, and scene management APIs.
pub mod scene;
/// Headless and integration-style test helpers.
pub mod test_support;
/// Miscellaneous utility traits, macros, and helper functions.
pub mod utils;

#[cfg(test)]
pub mod test_harness;
#[cfg(test)]
pub mod test_utils;

pub use engine_derive::*;
use inventory::collect;
use reflect::type_registry::TypeRegistry;

/// Inventory entry used by plugins and engine modules to register reflected
/// types at startup.
pub struct ReflectRegistrationFn {
    /// Debug-friendly name for the registration source.
    pub name: &'static str,
    /// Callback that inserts one or more types into a [`TypeRegistry`].
    pub function: fn(&mut TypeRegistry),
}
collect!(ReflectRegistrationFn);
