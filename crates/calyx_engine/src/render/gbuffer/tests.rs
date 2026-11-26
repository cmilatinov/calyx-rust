use super::GrassFieldUniform;
use crate::component::{ComponentGrassField, GrassBender, MAX_GRASS_BENDERS};
use crate::test_support::test_scene;
use nalgebra_glm::vec3;

#[test]
fn the_grass_field_reaches_the_shader_from_the_first_enabled_field() {
    let mut scene = test_scene();
    let still = GrassFieldUniform::of_scene(&scene, 2.0);
    assert_eq!((still.time, still.sway, still.bender_count), (2.0, 0.0, 0));

    let disabled = scene.create(None, None);
    scene.add_component(
        disabled,
        ComponentGrassField {
            sway: 9.0,
            ..Default::default()
        },
    );
    scene.set_component_enabled::<ComponentGrassField>(disabled, false);
    let field = scene.create(None, None);
    let bender = GrassBender {
        position: vec3(3.0, 1.0, -4.0),
        bend_radius: 2.0,
        reveal_radius: 2.5,
    };
    scene.add_component(
        field,
        ComponentGrassField {
            benders: vec![bender; MAX_GRASS_BENDERS + 4],
            ..Default::default()
        },
    );
    let uniform = GrassFieldUniform::of_scene(&scene, 1.5);
    assert_eq!(uniform.sway, ComponentGrassField::default().sway);
    assert_eq!(uniform.bender_count, MAX_GRASS_BENDERS as u32);
    assert_eq!(uniform.benders[0], [3.0, -4.0, 2.0, 2.5]);
    // The layout `shaders/mesh/grass_field.wgsl` declares.
    assert_eq!(size_of::<GrassFieldUniform>(), 32 + 16 * MAX_GRASS_BENDERS);
}

/// The transparent grass pass runs only while a bender in use fades the
/// grass.
#[test]
fn the_grass_fades_only_around_a_bender_in_use_that_reveals() {
    let mut uniform = GrassFieldUniform::default();
    assert!(!uniform.fades(), "no benders");
    uniform.benders[1] = [0.0, 0.0, 1.8, 3.2];
    uniform.bender_count = 1;
    assert!(!uniform.fades(), "the revealing bender is not in use");
    uniform.benders[0] = [0.0, 0.0, 1.8, 0.0];
    uniform.bender_count = 2;
    assert!(uniform.fades());
    uniform.benders[1][3] = 0.0;
    assert!(!uniform.fades(), "the benders only bend");
}
