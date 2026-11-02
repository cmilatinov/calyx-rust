// wgpu's nested handle types exceed the default trait-solver depth when
// checking Send/Sync on assets and components that hold GPU resources.
#![recursion_limit = "256"]

//! The demo scene collection's project assembly: the components the demo
//! scenes use that the engine does not ship.

pub mod camera_path;

use calyx_engine::reflect::type_registry::TypeRegistry;

#[no_mangle]
pub extern "C" fn plugin_main(registry: &mut TypeRegistry) {
    registry.register_crate(env!("CARGO_PKG_NAME"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera_path::ComponentCameraPath;
    use calyx_engine::component::ComponentTransform;

    #[test]
    fn plugin_main_exports_only_demo_reflection_types() {
        let mut registry = TypeRegistry {
            types: Default::default(),
            identities: Default::default(),
        };

        plugin_main(&mut registry);

        assert!(registry
            .type_registration::<ComponentCameraPath>()
            .is_some());
        assert!(registry.type_registration::<ComponentTransform>().is_none());
    }
}
