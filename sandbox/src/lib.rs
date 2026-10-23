// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

pub mod arena;
pub mod game_state;
pub mod hud;
pub mod sentry;
pub mod spawn;
pub mod tank;

use engine::reflect::type_registry::TypeRegistry;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    registry.register_crate(env!("CARGO_PKG_NAME"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ComponentArena;
    use crate::game_state::ComponentGameState;
    use crate::hud::ComponentTankHud;
    use crate::sentry::ComponentSentry;
    use crate::spawn::{ComponentRespawnState, ComponentSpawnPoint};
    use crate::tank::ComponentTankController;
    use engine::component::ComponentTransform;

    #[test]
    fn plugin_main_exports_only_sandbox_reflection_types() {
        let mut registry = TypeRegistry {
            types: Default::default(),
            identities: Default::default(),
        };

        plugin_main(&mut registry);

        assert!(registry
            .type_registration::<ComponentTankController>()
            .is_some());
        assert!(registry.type_registration::<ComponentArena>().is_some());
        assert!(registry.type_registration::<ComponentGameState>().is_some());
        assert!(registry.type_registration::<ComponentTankHud>().is_some());
        assert!(registry.type_registration::<ComponentSentry>().is_some());
        assert!(registry
            .type_registration::<ComponentSpawnPoint>()
            .is_some());
        assert!(registry
            .type_registration::<ComponentRespawnState>()
            .is_some());
        assert!(registry.type_registration::<ComponentTransform>().is_none());
    }
}
