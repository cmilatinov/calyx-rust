// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

//! The demo scene collection's project assembly. Components a demo needs
//! that the engine does not ship belong here.

use calyx_engine::reflect::type_registry::TypeRegistry;

pub mod terrain;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    registry.register_crate(env!("CARGO_PKG_NAME"));
}

#[cfg(test)]
mod tests;
