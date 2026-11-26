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
