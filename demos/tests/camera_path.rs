// Matches the crates under test: components holding GPU resources nest deep
// enough that proving them `Send`/`Sync` passes the default limit.
#![recursion_limit = "256"]

use calyx_engine::math::Transform;
use calyx_engine::scene::GameObjectRef;
use calyx_engine::test_support::{
    test_asset_context_with_assets, HeadlessSceneRunner, SceneBuilder,
};
use demos::camera_path::ComponentCameraPath;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};

fn stop(x: f32, z: f32) -> Transform {
    Transform::from_components(
        vec3(x, 2.0, z),
        UnitQuaternion::identity(),
        vec3(1.0, 1.0, 1.0),
    )
}

/// In play, the camera reaches each stop at its share of the loop: with four
/// stops and a four-second loop, it is on the second stop after one second.
#[test]
fn a_camera_path_flies_its_camera_through_the_stops_in_play() {
    let assets = test_asset_context_with_assets(Vec::new());
    demos::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());

    let mut builder = SceneBuilder::new(assets.registries.lock_read());
    let stops = builder.spawn("Camera Path");
    for (index, (x, z)) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        .iter()
        .enumerate()
    {
        let child = builder.spawn_child(stops, format!("Stop {}", index + 1));
        builder.set_transform(child, stop(*x, *z));
    }
    let camera = builder.spawn("Camera");
    let path = GameObjectRef::new(builder.scene().uuid(stops));
    builder.add_component(camera, ComponentCameraPath::new(path, 4.0));

    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.set_step_seconds(0.25);
    runner.step_many(4);

    let position = runner.scene().world_transform(camera).position;
    assert!(
        (position - Vec3::new(10.0, 2.0, 0.0)).norm() < 1e-3,
        "camera should be on the second stop, is at {position:?}"
    );
}
