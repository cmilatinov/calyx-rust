#![allow(clippy::module_inception)]
pub mod assets;
pub mod background;
pub mod class_registry;
pub mod component;
pub mod context;
pub mod core;
pub mod error;
pub mod ext;
pub mod input;
pub mod logging;
pub mod macros;
pub mod math;
pub mod net;
pub mod physics;
pub mod reflect;
pub mod render;
pub mod resource;
pub mod scene;
pub mod utils;

#[cfg(test)]
pub mod test_utils;
#[cfg(test)]
pub mod test_harness;

pub use engine_derive::*;
use component::{ComponentResetFn, ComponentUpdateFn};
use inventory::collect;
use reflect::type_registry::TypeRegistry;

pub struct ReflectRegistrationFn {
    pub name: &'static str,
    pub function: fn(&mut TypeRegistry),
}
collect!(ReflectRegistrationFn);

/// Registered by `#[derive(Component)]` for types with `#[reflect_attr(update)]`.
/// Collected via `inventory` and consumed by `ComponentRegistry::refresh_class_lists`.
pub struct ComponentUpdateRegistration {
    pub type_uuid: uuid::Uuid,
    pub update_fn: ComponentUpdateFn,
}
collect!(ComponentUpdateRegistration);

/// Registered by `#[derive(Component)]` for types with `#[reflect_attr(reset)]`.
pub struct ComponentResetRegistration {
    pub type_uuid: uuid::Uuid,
    pub reset_fn: ComponentResetFn,
}
collect!(ComponentResetRegistration);
