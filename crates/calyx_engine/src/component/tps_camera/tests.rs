use super::*;
use crate::test_support::test_registries;

/// Its update hook is its only consumer, and the scene skips that hook
/// for a disabled component, so the type opts in.
#[test]
fn a_third_person_camera_can_be_disabled() {
    let registries = test_registries();
    assert!(registries
        .components
        .read()
        .can_disable(ComponentThirdPersonCamera::type_uuid()));
}
