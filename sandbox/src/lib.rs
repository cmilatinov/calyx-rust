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
/// PROTOTYPE (#610): poise, knockdowns, the boot and the hook.
pub mod poise;
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
mod tests {
    use super::*;
    use crate::arena::ComponentArena;
    use crate::combat::{ComponentCombatStats, ComponentKillFeed};
    use crate::game_state::ComponentGameState;
    use crate::hud::ComponentTankHud;
    use crate::sentry::ComponentSentry;
    use crate::spawn::{ComponentRespawnState, ComponentSpawnPoint};
    use crate::tank::ComponentTankController;
    use crate::tank_camera::ComponentTankCamera;
    use calyx_engine::component::ComponentTransform;

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
        assert!(registry
            .type_registration::<crate::brush::ComponentBrush>()
            .is_some());
        assert!(registry
            .type_registration::<crate::brush::ComponentBrushReveal>()
            .is_some());
        assert!(registry.type_registration::<ComponentGameState>().is_some());
        assert!(registry
            .type_registration::<crate::match_rules::ComponentMatchRules>()
            .is_some());
        assert!(registry.type_registration::<ComponentTankHud>().is_some());
        assert!(registry.type_registration::<ComponentKillFeed>().is_some());
        assert!(registry
            .type_registration::<ComponentCombatStats>()
            .is_some());
        assert!(registry.type_registration::<ComponentSentry>().is_some());
        assert!(registry
            .type_registration::<ComponentSpawnPoint>()
            .is_some());
        assert!(registry
            .type_registration::<ComponentRespawnState>()
            .is_some());
        assert!(registry
            .type_registration::<ComponentTankCamera>()
            .is_some());
        assert!(registry
            .type_registration::<crate::track_marks::ComponentTrackMarks>()
            .is_some());
        assert!(registry
            .type_registration::<crate::poise::ComponentPoise>()
            .is_some());
        assert!(registry
            .type_registration::<crate::poise::ComponentDummyDriver>()
            .is_some());
        assert!(registry
            .type_registration::<crate::poise::abilities::ComponentTankAbilities>()
            .is_some());
        assert!(registry
            .type_registration::<crate::poise::hud::ComponentPoiseHud>()
            .is_some());
        assert!(registry.type_registration::<ComponentTransform>().is_none());
    }
}
