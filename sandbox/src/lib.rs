pub mod game_state;
pub mod spawn;
mod tank;

use engine::reflect::type_registry::TypeRegistry;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    for f in inventory::iter::<engine::ReflectRegistrationFn>() {
        if f.crate_name != env!("CARGO_PKG_NAME") {
            continue;
        }
        (f.function)(registry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_state::ComponentGameState;
    use crate::spawn::{ComponentRespawnState, ComponentSpawnPoint};
    use crate::tank::ComponentTankController;
    use engine::component::ComponentTransform;

    #[test]
    fn plugin_main_exports_only_sandbox_reflection_types() {
        let mut registry = TypeRegistry {
            types: Default::default(),
        };

        plugin_main(&mut registry);

        assert!(registry
            .type_registration::<ComponentTankController>()
            .is_some());
        assert!(registry.type_registration::<ComponentGameState>().is_some());
        assert!(registry
            .type_registration::<ComponentSpawnPoint>()
            .is_some());
        assert!(registry
            .type_registration::<ComponentRespawnState>()
            .is_some());
        assert!(registry.type_registration::<ComponentTransform>().is_none());
    }
}
