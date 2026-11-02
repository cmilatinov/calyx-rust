// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

//! The demo scene collection's project assembly. The demos use only engine
//! components today; components a demo needs that the engine does not ship
//! belong here.

use calyx_engine::reflect::type_registry::TypeRegistry;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    registry.register_crate(env!("CARGO_PKG_NAME"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use calyx_engine::component::{ComponentCameraPath, ComponentTransform};

    #[test]
    fn plugin_main_exports_no_engine_types() {
        let mut registry = TypeRegistry {
            types: Default::default(),
            identities: Default::default(),
        };

        plugin_main(&mut registry);

        assert!(registry.type_registration::<ComponentTransform>().is_none());
        assert!(registry
            .type_registration::<ComponentCameraPath>()
            .is_none());
    }
}
