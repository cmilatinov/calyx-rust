//! Core engine runtime, asset pipeline, editor-facing reflection, and support
//! systems shared across the Calyx workspace.

#![allow(clippy::module_inception)]
// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]
extern crate self as calyx_engine;
// Linked only for the features it pins; see crates/calyx_engine_feature_pins/Cargo.toml.
use calyx_engine_feature_pins as _;

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
/// Game runtime setup shared by the standalone player and the editor.
pub mod runtime;
/// Scene graphs, game objects, prefabs, and scene management APIs.
pub mod scene;
/// Headless and integration-style test helpers.
pub mod test_support;
/// Backend-neutral runtime UI and HUD composition APIs.
pub mod ui;
/// Miscellaneous utility traits, macros, and helper functions.
pub mod utils;

pub use calyx_engine_derive::*;
pub use reflect::type_registry::ReflectRegistrationFn;
