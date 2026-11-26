//! A component that flies its game object, usually a camera, along a path
//! whose stops are the children of a path object.

use crate as calyx_engine;
use crate::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use crate::input::Input;
use crate::math::{self, Transform};
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
    fn has_gizmos(&self) -> bool {
        true
    }

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
            let segments = if self.looping {
                stops.len()
            } else {
                stops.len() - 1
            };
            for index in 0..segments {
                let [p0, p1, p2, p3] = segment_points(&stops, index, self.looping);
                gizmos.catmull_rom(&p0, &p1, &p2, &p3, GIZMO_SEGMENTS_PER_STOP);
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
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let Some(follower) = this.read(scene, |c| c.clone()) else {
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
                this.write(scene, |c| c.warned = true);
            }
            return;
        }
        let elapsed = follower.elapsed + resources.time().delta_time().max(0.0);
        this.write(scene, |c| c.elapsed = elapsed);
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
    let [p0, p1, p2, p3] = segment_points(stops, index, looping);
    let position = math::catmull_rom(p0, p1, p2, p3, u);
    let (from, to) = (stops[index], stops[(index + 1) % count]);
    let rotation = from.rotation.slerp(&to.rotation, smoothstep(u));
    Some(Transform::from_components(position, rotation, from.scale))
}

/// The positions of segment `index`'s stop, the next stop and their
/// neighbours, as the Catmull-Rom curve needs them: a looping path wraps
/// round, and an open path's end stop stands in for its missing neighbour.
fn segment_points(stops: &[Transform], index: usize, looping: bool) -> [Vec3; 4] {
    let count = stops.len() as isize;
    let at = |offset: isize| {
        let i = index as isize + offset;
        let i = if looping {
            i.rem_euclid(count)
        } else {
            i.clamp(0, count - 1)
        };
        stops[i as usize].position
    };
    [at(-1), at(0), at(1), at(2)]
}

/// Eases the turn between two stops so the follower does not snap round at
/// each one.
fn smoothstep(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

#[cfg(test)]
mod tests;
