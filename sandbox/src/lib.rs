// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

pub mod arena;
pub mod brush;
pub mod collision;
pub mod combat;
pub mod combat_vfx;
pub mod game_state;
pub mod hud;
pub mod match_rules;
pub mod sentry;
pub mod spawn;
pub mod tank;
pub mod tank_camera;
pub mod terrain;
pub mod track_marks;
pub mod vision;

use calyx_engine::reflect::type_registry::TypeRegistry;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    registry.register_crate(env!("CARGO_PKG_NAME"));
}

#[cfg(test)]
mod tests;
