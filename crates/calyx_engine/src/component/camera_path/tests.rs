use super::*;
use crate::test_support::{test_registries, HeadlessSceneRunner, SceneBuilder};
use nalgebra::UnitQuaternion;
use nalgebra_glm::vec3;

fn waypoint(x: f32, y: f32, z: f32, yaw_degrees: f32) -> Transform {
    Transform::from_components(
        vec3(x, y, z),
        UnitQuaternion::from_euler_angles(0.0, yaw_degrees.to_radians(), 0.0),
        vec3(1.0, 1.0, 1.0),
    )
}

fn square() -> Vec<Transform> {
    vec![
        waypoint(0.0, 2.0, 0.0, 0.0),
        waypoint(10.0, 2.0, 0.0, 90.0),
        waypoint(10.0, 2.0, 10.0, 180.0),
        waypoint(0.0, 2.0, 10.0, 270.0),
    ]
}

#[test]
fn an_empty_path_has_no_pose() {
    assert!(sample_path(&[], 0.3, true).is_none());
}

#[test]
fn a_looping_path_passes_through_each_stop_facing_its_way() {
    let path = square();
    for (index, expected) in path.iter().enumerate() {
        let pose = sample_path(&path, index as f32 / 4.0, true).unwrap();
        assert!((pose.position - expected.position).norm() < 1e-4);
        assert!(pose.rotation.angle_to(&expected.rotation) < 1e-3);
    }
}

#[test]
fn an_open_path_runs_from_the_first_stop_to_the_last_and_holds() {
    let path = square();
    for (index, expected) in path.iter().enumerate() {
        let pose = sample_path(&path, index as f32 / 3.0, false).unwrap();
        assert!((pose.position - expected.position).norm() < 1e-4);
        assert!(pose.rotation.angle_to(&expected.rotation) < 1e-3);
    }
    let after = sample_path(&path, 1.7, false).unwrap();
    assert!((after.position - path[3].position).norm() < 1e-4);
}

#[test]
fn a_looping_path_loops_seamlessly() {
    let path = square();
    let start = sample_path(&path, 0.0, true).unwrap();
    let wrapped = sample_path(&path, 1.0, true).unwrap();
    let next_loop = sample_path(&path, 2.25, true).unwrap();
    assert!((wrapped.position - start.position).norm() < 1e-4);
    assert!((next_loop.position - path[1].position).norm() < 1e-4);
    // Either side of the seam the follower is next to the first stop,
    // not jumping across the loop.
    let before = sample_path(&path, 0.999, true).unwrap();
    let after = sample_path(&path, 0.001, true).unwrap();
    assert!((before.position - after.position).norm() < 0.2);
    assert!(before.rotation.angle_to(&after.rotation) < 0.01);
}

#[test]
fn between_two_stops_the_curve_bulges_and_the_turn_is_halfway() {
    // Catmull-Rom through the square's corners: halfway along the first
    // side it bulges outwards, below the straight line z = 0.
    let path = square();
    let pose = sample_path(&path, 0.125, true).unwrap();
    assert!((pose.position.x - 5.0).abs() < 1e-4);
    assert!((pose.position.y - 2.0).abs() < 1e-4);
    assert!((pose.position.z - -1.25).abs() < 1e-4);
    let halfway = UnitQuaternion::from_euler_angles(0.0, 45f32.to_radians(), 0.0);
    assert!(pose.rotation.angle_to(&halfway) < 1e-3);
}

/// A scene with a `Camera Path` parent holding the square's stops, in
/// that hierarchy order, and `followers` objects following it.
fn scene_with_path(followers: usize) -> (Scene, Vec<GameObject>) {
    let mut builder = SceneBuilder::new(test_registries());
    let parent = builder.spawn("Camera Path");
    for (index, stop) in square().into_iter().enumerate() {
        let child = builder.spawn_child(parent, format!("Stop {}", index + 1));
        builder.set_transform(child, stop.matrix());
    }
    let path = GameObjectRef::new(builder.scene().uuid(parent));
    let cameras = (0..followers)
        .map(|index| {
            let camera = builder.spawn(format!("Camera {index}"));
            builder.add_component(camera, ComponentCameraPath::new(path, 4.0, true));
            camera
        })
        .collect();
    (builder.finish(), cameras)
}

/// In play, the follower reaches each stop at its share of the loop: with
/// four stops and a four-second loop, it is on the second stop, facing
/// its way, after one second.
#[test]
fn a_camera_path_flies_its_camera_through_the_stops_in_play() {
    let (scene, cameras) = scene_with_path(1);
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(0.25);
    runner.step_many(4);

    let transform = runner.scene().world_transform(cameras[0]);
    assert!(
        (transform.position - vec3(10.0, 2.0, 0.0)).norm() < 1e-3,
        "camera should be on the second stop, is at {:?}",
        transform.position
    );
    let facing = UnitQuaternion::from_euler_angles(0.0, 90f32.to_radians(), 0.0);
    assert!(transform.rotation.angle_to(&facing) < 1e-3);
}

#[test]
fn two_cameras_can_follow_one_path() {
    let (scene, cameras) = scene_with_path(2);
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(0.25);
    runner.step_many(8);

    for camera in cameras {
        let position = runner.scene().world_transform(camera).position;
        assert!((position - vec3(10.0, 2.0, 10.0)).norm() < 1e-3);
    }
}

#[test]
fn a_path_with_one_stop_leaves_the_camera_where_it_is() {
    let mut builder = SceneBuilder::new(test_registries());
    let parent = builder.spawn("Camera Path");
    let child = builder.spawn_child(parent, "Stop");
    builder.set_transform(child, waypoint(5.0, 5.0, 5.0, 0.0).matrix());
    let camera = builder.spawn("Camera");
    let path = GameObjectRef::new(builder.scene().uuid(parent));
    builder.add_component(camera, ComponentCameraPath::new(path, 4.0, true));
    let mut runner = HeadlessSceneRunner::from_scene(builder.finish());
    runner.step_many(4);
    assert_eq!(
        runner.scene().world_transform(camera).position,
        Vec3::zeros()
    );
}

/// Line vertices the path's gizmo draws for `follower` with `selected`
/// selected in the editor.
fn gizmo_line_vertices(scene: &Scene, follower: GameObject, selected: Option<GameObject>) -> usize {
    use crate::assets::mesh::Mesh;
    use crate::render::{Camera, GizmoOptions};
    let render_context = crate::test_support::test_render_context();
    let camera = Camera::default();
    let camera_transform = Transform::default();
    let (mut circles, mut cubes) = (Vec::new(), Vec::new());
    let mut lines = Mesh::new(&render_context);
    let mut points = Mesh::new(&render_context);
    let mut icons = Mesh::new(&render_context);
    let mut gizmos = Gizmos {
        camera: &camera,
        camera_transform: &camera_transform,
        viewport_size: [1920.0, 1080.0],
        options: GizmoOptions {
            selected: selected.map(|object| scene.uuid(object)),
            ..Default::default()
        },
        object_id: 0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circles,
        cube_list: &mut cubes,
        lines_mesh: &mut lines,
        points_mesh: &mut points,
        icons_mesh: &mut icons,
    };
    let path = scene
        .read_component::<ComponentCameraPath, _, _>(follower, |c| c.clone())
        .unwrap();
    path.draw_gizmos(scene, follower, &mut gizmos);
    lines.vertices.len()
}

/// The gizmo draws while the follower, the path object or a stop is
/// selected, and not while something else is.
#[test]
fn the_path_gizmo_draws_while_the_path_or_its_follower_is_selected() {
    let (scene, cameras) = scene_with_path(2);
    let follower = cameras[0];
    let parent = scene
        .objects()
        .find(|o| scene.name(*o) == "Camera Path")
        .unwrap();
    let stop = scene.children_ordered(parent).nth(2).unwrap();
    assert_eq!(gizmo_line_vertices(&scene, follower, None), 0);
    assert_eq!(gizmo_line_vertices(&scene, follower, Some(cameras[1])), 0);
    for selected in [follower, parent, stop] {
        assert!(gizmo_line_vertices(&scene, follower, Some(selected)) > 0);
    }
}
