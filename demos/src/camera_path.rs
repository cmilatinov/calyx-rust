//! A camera that flies a looping path through waypoint objects.
//!
//! [`ComponentCameraPath`] sits on the camera's game object. Its `waypoints`
//! object's children, in sibling order, are the stops: the camera passes
//! through each child's world position facing the way that child does, and
//! after the last child flies back to the first. Each stop takes the same
//! share of `loop_seconds`. The path runs only in play, so in the editor the
//! camera keeps its authored framing.

use calyx_engine::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::GameObjectRef;
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

/// Flies its game object along the looping path its `waypoints` object's
/// children mark.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b6f1c0de-2a4e-4c5b-9d37-8e1f4a6b2c90"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Camera Path")]
#[serde(default)]
pub struct ComponentCameraPath {
    /// The object whose children, in sibling order, are the path's stops.
    pub waypoints: GameObjectRef,
    /// Seconds one loop through every stop takes.
    #[reflect_attr(speed = 0.1, min = 0.1)]
    pub loop_seconds: f32,
    #[serde(skip)]
    #[reflect_skip]
    elapsed: f32,
}

impl Default for ComponentCameraPath {
    fn default() -> Self {
        Self {
            waypoints: Default::default(),
            loop_seconds: 60.0,
            elapsed: 0.0,
        }
    }
}

impl ComponentCameraPath {
    /// A path through `waypoints`' children that loops every `loop_seconds`.
    pub fn new(waypoints: GameObjectRef, loop_seconds: f32) -> Self {
        Self {
            waypoints,
            loop_seconds,
            ..Default::default()
        }
    }
}

impl Component for ComponentCameraPath {
    /// A disabled camera path skips its update hook, so the camera stays put.
    fn can_disable(&self) -> bool {
        true
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
        // The hook runs on the registered prototype, so the path's own state
        // is read from the scene.
        let Some(path) = scene.read_component::<ComponentCameraPath, _, _>(game_object, |c| *c)
        else {
            return;
        };
        let elapsed = path.elapsed + resources.time().delta_time().max(0.0);
        let _ = scene.write_component::<ComponentCameraPath, _>(game_object, |c| {
            c.elapsed = elapsed;
        });
        let Some(parent) = path.waypoints.game_object(scene) else {
            return;
        };
        let stops: Vec<Transform> = scene
            .children_ordered(parent)
            .map(|stop| scene.world_transform(stop))
            .collect();
        let Some(pose) = sample_path(&stops, elapsed / path.loop_seconds.max(0.1)) else {
            return;
        };
        let mut transform = scene.world_transform(game_object);
        transform.position = pose.position;
        transform.rotation = pose.rotation;
        scene.set_world_transform(game_object, transform.matrix());
    }
}

/// The pose a fraction `t` of the way around the closed path through `stops`:
/// a Catmull-Rom curve through their positions, turning from each stop's
/// rotation to the next's. Each stop takes an equal share of the loop, so
/// `t = i / n` is stop `i`, and whole loops wrap. `None` without stops.
pub fn sample_path(stops: &[Transform], t: f32) -> Option<Transform> {
    let count = stops.len();
    let first = stops.first()?;
    if count == 1 {
        return Some(*first);
    }
    let along = t.rem_euclid(1.0) * count as f32;
    let index = (along.floor() as usize).min(count - 1);
    let u = along - index as f32;
    let at = |offset: isize| stops[(index as isize + offset).rem_euclid(count as isize) as usize];
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

/// Eases the turn between two stops so the camera does not snap round at
/// each one.
fn smoothstep(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(sample_path(&[], 0.3).is_none());
    }

    #[test]
    fn the_path_passes_through_each_waypoint_at_its_share_of_the_loop() {
        let path = square();
        for (index, expected) in path.iter().enumerate() {
            let pose = sample_path(&path, index as f32 / 4.0).unwrap();
            assert!((pose.position - expected.position).norm() < 1e-4);
            assert!(pose.rotation.angle_to(&expected.rotation) < 1e-3);
        }
    }

    #[test]
    fn the_path_loops_back_to_the_first_waypoint() {
        let path = square();
        let start = sample_path(&path, 0.0).unwrap();
        let wrapped = sample_path(&path, 1.0).unwrap();
        let next_loop = sample_path(&path, 2.25).unwrap();
        assert!((wrapped.position - start.position).norm() < 1e-4);
        assert!((next_loop.position - path[1].position).norm() < 1e-4);
    }

    #[test]
    fn between_two_waypoints_the_camera_is_on_a_smooth_curve_and_turns_halfway() {
        // Catmull-Rom through the square's corners: halfway along the first
        // side it bulges outwards, below the straight line z = 0.
        let path = square();
        let pose = sample_path(&path, 0.125).unwrap();
        assert!((pose.position.x - 5.0).abs() < 1e-4);
        assert!((pose.position.y - 2.0).abs() < 1e-4);
        assert!((pose.position.z - -1.25).abs() < 1e-4);
        let halfway = UnitQuaternion::from_euler_angles(0.0, 45f32.to_radians(), 0.0);
        assert!(pose.rotation.angle_to(&halfway) < 1e-3);
    }

    #[test]
    fn a_single_waypoint_holds_the_camera_still() {
        let path = vec![waypoint(1.0, 2.0, 3.0, 30.0)];
        let pose = sample_path(&path, 0.6).unwrap();
        assert!((pose.position - vec3(1.0, 2.0, 3.0)).norm() < 1e-5);
    }
}
