//! A component that flies its game object, usually a camera, along a path
//! whose stops are the children of a path object.

use crate as calyx_engine;
use crate::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use crate::input::Input;
use crate::math::Transform;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::resource::ResourceMap;
use crate::scene::{GameObject, GameObjectRef, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

/// Line segments the gizmo draws between two stops.
const GIZMO_SEGMENTS_PER_STOP: usize = 16;
/// Radius of the gizmo's marker at each stop.
const GIZMO_STOP_RADIUS: f32 = 0.4;
/// Length of the gizmo's line showing which way each stop points.
const GIZMO_FACING_LENGTH: f32 = 1.5;

/// Flies its game object, usually a camera, along the path its `path` object
/// marks, in play.
///
/// The path's stops are the `path` object's children, in hierarchy order:
/// the object passes through each child's world position on a Catmull-Rom
/// curve, turning from each child's world rotation to the next with an eased
/// slerp, so a camera on the path faces the way each stop points as it
/// passes it. Adding, reordering or removing stops is done in the
/// hierarchy, and moving the `path` object moves the whole path. Each
/// stop-to-stop segment takes an equal share of `duration`. A looping path
/// flies back from the last stop to the first and repeats; otherwise the
/// object stops at the last stop. A path of fewer than two stops does
/// nothing and warns once. Several objects can follow one path. The path
/// only runs in play, so in the editor the object keeps its authored
/// transform; while the follower, the path object or one of its stops is
/// selected, the gizmo shows the curve.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b6f1c0de-2a4e-4c5b-9d37-8e1f4a6b2c90"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Camera Path", category = "Camera")]
#[serde(default)]
pub struct ComponentCameraPath {
    /// The object whose children, in hierarchy order, are the path's stops.
    pub path: GameObjectRef,
    /// Seconds one pass through every stop takes.
    #[reflect_attr(speed = 0.1, min = 0.1)]
    pub duration: f32,
    /// Whether the path closes back to its first stop and repeats.
    pub looping: bool,
    #[serde(skip)]
    #[reflect_skip]
    elapsed: f32,
    #[serde(skip)]
    #[reflect_skip]
    warned: bool,
}

impl Default for ComponentCameraPath {
    fn default() -> Self {
        Self {
            path: Default::default(),
            duration: 60.0,
            looping: true,
            elapsed: 0.0,
            warned: false,
        }
    }
}

impl ComponentCameraPath {
    /// Follows the children of `path` over `duration` seconds.
    pub fn new(path: GameObjectRef, duration: f32, looping: bool) -> Self {
        Self {
            path,
            duration,
            looping,
            ..Default::default()
        }
    }

    /// The path object and its stops, in hierarchy order.
    fn stops(&self, scene: &Scene) -> Option<(GameObject, Vec<GameObject>)> {
        let parent = self.path.game_object(scene)?;
        Some((parent, scene.children_ordered(parent).collect()))
    }
}

impl Component for ComponentCameraPath {
    /// A disabled camera path skips its update hook, so the object stays put.
    fn can_disable(&self) -> bool {
        true
    }

    /// While the follower, the path object or one of its stops is selected:
    /// the curve the follower flies, a marker at each stop, and a line
    /// showing which way each stop points.
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let Some((parent, stop_objects)) = self.stops(scene) else {
            return;
        };
        let Some(selected) = gizmos.selected_object() else {
            return;
        };
        let related = [game_object, parent]
            .into_iter()
            .chain(stop_objects.iter().copied())
            .any(|object| scene.uuid(object) == selected);
        if !related {
            return;
        }
        let stops: Vec<Transform> = stop_objects
            .iter()
            .map(|stop| scene.world_transform(*stop))
            .collect();
        gizmos.set_color(&Vec4::new(1.0, 0.8, 0.2, 1.0));
        if stops.len() >= 2 {
            let samples = stops.len() * GIZMO_SEGMENTS_PER_STOP;
            let mut previous = stops[0].position;
            for k in 1..=samples {
                let t = k as f32 / samples as f32;
                if let Some(pose) = sample_path(&stops, t, self.looping) {
                    gizmos.line(&previous, &pose.position);
                    previous = pose.position;
                }
            }
        }
        for stop in &stops {
            gizmos.wire_sphere(&stop.position, GIZMO_STOP_RADIUS);
            let ahead = stop.position + stop.forward().normalize() * GIZMO_FACING_LENGTH;
            gizmos.line(&stop.position, &ahead);
        }
    }
}

impl ComponentUpdate for ComponentCameraPath {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        // The hook runs on the registry's prototype, not this object's
        // component (#481), so the path and its progress are read from the
        // scene.
        let Some(follower) =
            scene.read_component::<ComponentCameraPath, _, _>(game_object, |c| c.clone())
        else {
            return;
        };
        let stops: Vec<Transform> = follower
            .stops(scene)
            .map(|(_, stops)| stops.iter().map(|s| scene.world_transform(*s)).collect())
            .unwrap_or_default();
        if stops.len() < 2 {
            if !follower.warned {
                log::warn!(
                    "The camera path on {} needs a path object with at least two child stops; \
                     it does nothing",
                    scene.name(game_object)
                );
                let _ = scene.write_component::<ComponentCameraPath, _>(game_object, |c| {
                    c.warned = true;
                });
            }
            return;
        }
        let elapsed = follower.elapsed + resources.time().delta_time().max(0.0);
        let _ = scene.write_component::<ComponentCameraPath, _>(game_object, |c| {
            c.elapsed = elapsed;
        });
        let t = elapsed / follower.duration.max(0.1);
        let Some(pose) = sample_path(&stops, t, follower.looping) else {
            return;
        };
        let mut transform = scene.world_transform(game_object);
        transform.position = pose.position;
        transform.rotation = pose.rotation;
        scene.set_world_transform(game_object, transform.matrix());
    }
}

/// The pose a fraction `t` of the way along the path through `stops`: a
/// Catmull-Rom curve through their positions, turning from each stop's
/// rotation to the next's with an eased slerp. Each stop-to-stop segment
/// takes an equal share of the path. A looping path closes back to the first
/// stop, so `t = i / n` is stop `i` and whole loops wrap; an open path runs
/// from the first stop at `t = 0` to the last at `t = 1` and holds there.
/// `None` without stops.
pub fn sample_path(stops: &[Transform], t: f32, looping: bool) -> Option<Transform> {
    let count = stops.len();
    let first = stops.first()?;
    if count == 1 {
        return Some(*first);
    }
    let segments = if looping { count } else { count - 1 };
    let t = if looping {
        t.rem_euclid(1.0)
    } else {
        t.clamp(0.0, 1.0)
    };
    let along = t * segments as f32;
    let index = (along.floor() as usize).min(segments - 1);
    let u = along - index as f32;
    let at = |offset: isize| {
        let i = index as isize + offset;
        let i = if looping {
            i.rem_euclid(count as isize)
        } else {
            i.clamp(0, count as isize - 1)
        };
        stops[i as usize]
    };
    let (p0, p1, p2, p3) = (at(-1), at(0), at(1), at(2));
    let position = catmull_rom(p0.position, p1.position, p2.position, p3.position, u);
    let rotation = p1.rotation.slerp(&p2.rotation, smoothstep(u));
    Some(Transform::from_components(position, rotation, p1.scale))
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let u2 = u * u;
    let u3 = u2 * u;
    (p1 * 2.0
        + (p2 - p0) * u
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * u3)
        * 0.5
}

/// Eases the turn between two stops so the follower does not snap round at
/// each one.
fn smoothstep(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests {
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
    fn gizmo_line_vertices(
        scene: &Scene,
        follower: GameObject,
        selected: Option<GameObject>,
    ) -> usize {
        use crate::assets::mesh::Mesh;
        use crate::render::Camera;
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
            selected_object: selected.map(|object| scene.uuid(object)),
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
}
