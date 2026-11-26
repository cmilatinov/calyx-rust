use super::*;
use crate::math::Transform;
use crate::scene::SceneData;
use crate::test_support::{test_registries, TestAssets};
use approx::assert_abs_diff_eq;
use image::{ImageBuffer, Luma};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3};

#[test]
fn terrain_is_registered_and_round_trips_through_a_scene() {
    let registries = test_registries();
    assert!(registries
        .components
        .read()
        .component(ComponentTerrain::type_uuid())
        .is_some());
    let mut scene = registries.scene();
    let owner = scene.create(None, None);
    let heightmap = uuid::Uuid::new_v4();
    scene.add_component(
        owner,
        ComponentTerrain {
            heightmap: AssetRef::from_id(heightmap),
            size: vec2(64.0, 32.0),
            height_scale: 7.5,
            cast_shadows: false,
            ..Default::default()
        },
    );
    let data: SceneData = (&scene).into();
    let json = serde_json::to_value(&data).unwrap();
    let restored: SceneData = serde_json::from_value(json).unwrap();
    let value = &restored.components[&scene.uuid(owner)][&ComponentTerrain::type_uuid()];
    let terrain: ComponentTerrain = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(terrain.heightmap, AssetRef::<Texture>::from_id(heightmap));
    assert_eq!(terrain.size, vec2(64.0, 32.0));
    assert_eq!(terrain.height_scale, 7.5);
    assert!(!terrain.cast_shadows);
}

#[test]
fn world_samples_follow_the_owner_transform() {
    // A ramp rising 1 per column along local +X over 3 columns.
    let field = Heightfield::new(3, 2, vec![0.0, 0.5, 1.0, 0.0, 0.5, 1.0]).unwrap();
    let extent = HeightfieldExtent {
        size: vec2(4.0, 4.0),
        height_scale: 2.0,
    };
    // Moved, turned a quarter around Y (local +X to world -Z) and scaled.
    let world = Transform::from_components(
        vec3(10.0, 5.0, -3.0),
        UnitQuaternion::from_axis_angle(&Vec3::y_axis(), std::f32::consts::FRAC_PI_2),
        vec3(2.0, 3.0, 2.0),
    )
    .matrix();
    // World Z -3 - 2 * 1 is local X 1, three quarters up the ramp.
    let sample = sample_at(&world, &field, &extent, vec2(10.0, -5.0)).unwrap();
    assert_abs_diff_eq!(sample.height, 5.0 + 3.0 * 1.5, epsilon = 1e-4);
    // The ramp climbs toward world -Z, so its normal leans toward +Z.
    assert!(sample.normal.z > 0.0 && sample.normal.y > 0.0);
    assert_abs_diff_eq!(sample.normal.x, 0.0, epsilon = 1e-5);
    assert_abs_diff_eq!(sample.normal.norm(), 1.0, epsilon = 1e-5);
    // The local slope of 0.5 per unit, stretched by the scale to 0.5 * 3 / 2.
    assert_abs_diff_eq!(sample.normal.z / sample.normal.y, 0.75, epsilon = 1e-4);
    // Outside the scaled 8 x 8 rectangle.
    assert!(sample_at(&world, &field, &extent, vec2(10.0, -7.5)).is_none());
}

#[test]
fn gameplay_samples_a_terrain_through_its_heightmap_asset() {
    // The texture's upload needs the engine's mip shaders.
    let assets = TestAssets::engine();
    let path = assets.engine_root().join("ramp.png");
    // 16-bit ramp: 0 on the left column, 1 on the right.
    ImageBuffer::from_fn(5, 5, |x, _| Luma([(x * 65535 / 4) as u16]))
        .save(&path)
        .unwrap();
    let registries = assets.registries();
    let heightmap = registries.assets.read().asset_id_from_path(&path).unwrap();
    let mut scene = registries.scene();
    let terrain = scene.create(None, None);
    scene.add_component(
        terrain,
        ComponentTerrain {
            heightmap: AssetRef::from_id(heightmap),
            size: vec2(8.0, 8.0),
            height_scale: 4.0,
            ..Default::default()
        },
    );
    scene.set_transform(terrain, &glm::translation(&vec3(0.0, 1.0, 0.0)));
    let sample = ComponentTerrain::sample(&scene, terrain, vec2(2.0, 0.0)).unwrap();
    assert_abs_diff_eq!(sample.height, 1.0 + 3.0, epsilon = 1e-4);
    assert!(sample.normal.x < 0.0);
    let any = ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)).unwrap();
    assert_eq!(any, sample);
    assert!(ComponentTerrain::sample_scene(&scene, vec2(20.0, 0.0)).is_none());

    // A second, higher terrain over part of it wins where both cover.
    let higher = scene.create(None, None);
    scene.add_component(
        higher,
        ComponentTerrain {
            heightmap: AssetRef::from_id(heightmap),
            size: vec2(2.0, 2.0),
            height_scale: 4.0,
            ..Default::default()
        },
    );
    scene.set_transform(higher, &glm::translation(&vec3(2.0, 10.0, 0.0)));
    let top = ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)).unwrap();
    assert_abs_diff_eq!(top.height, 10.0 + 2.0, epsilon = 1e-4);
    scene.set_component_enabled::<ComponentTerrain>(higher, false);
    assert_eq!(
        ComponentTerrain::sample_scene(&scene, vec2(2.0, 0.0)),
        Some(sample)
    );
    assert!(ComponentTerrain::sample(&scene, higher, vec2(2.0, 0.0)).is_some());
}
