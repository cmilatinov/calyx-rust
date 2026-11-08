//! The ground: drive surfaces, the floor, ramps and platform decks tanks
//! ride on and the cursor aims at, and the terrain's relief, which tanks,
//! the cursor and rounds treat the same way. Everything else with a static
//! collider is an obstacle that stops the hull.
//!
//! The terrain has no collider. Gameplay asks for its ground by sampling
//! the drawn surface through [`ComponentTerrain::sample_scene`] and
//! [`ComponentTerrain::sample`], so what tanks settle on, what the cursor
//! aims at and what stops a round is the surface the player sees, triangle
//! for triangle.

use calyx_engine::component::{Component, ComponentCollider, ComponentTerrain, ReflectComponent};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::Ray;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::{Entity, IntoQuery};
use nalgebra_glm::{vec2, vec3, vec4, Vec3};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Marks a static collider whose top tanks drive on. The hull never
/// collides with it; instead tanks settle on its top, and the cursor aims at
/// the point on it under the cursor. Rounds still hit it like any other
/// static collider.
#[derive(Clone, Copy, Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5b0f8c62-3d1e-4a7b-9c55-2e8d4f71a903"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Drive Surface")]
#[serde(default)]
pub struct ComponentDriveSurface {}

impl Component for ComponentDriveSurface {}

/// Entities whose colliders are drive surfaces.
pub(crate) fn drive_surfaces(scene: &Scene) -> HashSet<Entity> {
    scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentDriveSurface, _, _>(object, |_| ())
                .is_some()
                && scene
                    .read_component::<ComponentCollider, _, _>(object, |_| ())
                    .is_some()
        })
        .map(|object| object.entity)
        .collect()
}

/// The first point on a drive surface along `origin` + `direction` * t,
/// for t up to `max_distance`.
fn first_surface_hit(
    scene: &Scene,
    surfaces: &HashSet<Entity>,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<Vec3> {
    if surfaces.is_empty() {
        return None;
    }
    let is_surface = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| surfaces.contains(&entity))
    };
    scene
        .physics
        .cast_ray(
            origin,
            direction,
            max_distance,
            true,
            QueryFilter::default().predicate(&is_surface),
        )
        .map(|hit| hit.point)
}

/// Height of the highest ground under `point` within `below` metres of
/// it, looking down from `point` itself: a drive surface or the terrain.
pub(crate) fn surface_height_below(
    scene: &Scene,
    surfaces: &HashSet<Entity>,
    point: Vec3,
    below: f32,
) -> Option<f32> {
    let surface =
        first_surface_hit(scene, surfaces, point, vec3(0.0, -1.0, 0.0), below).map(|hit| hit.y);
    let terrain = ComponentTerrain::sample_scene(scene, vec2(point.x, point.z))
        .map(|sample| sample.height)
        .filter(|&height| height <= point.y && height >= point.y - below);
    match (surface, terrain) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// Where `ray`, such as the cursor's, first meets the ground: a drive
/// surface or the terrain, whichever it reaches first.
pub(crate) fn aimed_surface_point(scene: &Scene, ray: &Ray) -> Option<Vec3> {
    const REACH: f32 = 1000.0;
    let surface = first_surface_hit(
        scene,
        &drive_surfaces(scene),
        ray.origin,
        ray.direction,
        REACH,
    );
    let end = ray.origin + ray.direction * REACH;
    let terrain = terrain_hit_time(scene, ray.origin, end, 0.0)
        .map(|time| ray.origin + (end - ray.origin) * time);
    match (surface, terrain) {
        (Some(a), Some(b)) => Some(
            if (a - ray.origin).magnitude_squared() <= (b - ray.origin).magnitude_squared() {
                a
            } else {
                b
            },
        ),
        (a, b) => a.or(b),
    }
}

/// How far apart the terrain is sampled along a path, in metres: half the
/// arena terrain's half-metre cells, so a path cannot step over a rise.
const TERRAIN_MARCH_STEP: f32 = 0.25;
/// Halvings that place a crossing found between two samples.
const TERRAIN_REFINE_STEPS: usize = 10;

/// The fraction of the `start`..`end` path a round of `radius` travels
/// before it touches an enabled terrain, if it does: where its centre comes
/// within `radius` of the surface straight below it. A path that starts
/// under the surface touches it at once.
///
/// The path is sampled every [`TERRAIN_MARCH_STEP`] only where it passes
/// through a terrain's bounds, so a path well above the relief, as a round
/// at gun height is, costs no samples.
pub(crate) fn terrain_hit_time(scene: &Scene, start: Vec3, end: Vec3, radius: f32) -> Option<f32> {
    let lift = vec3(0.0, radius, 0.0);
    let (start, end) = (start - lift, end - lift);
    let length = (end - start).magnitude();
    // Without a terrain, skip looking for one among the objects.
    <&ComponentTerrain>::query().iter(scene.world()).next()?;
    scene
        .objects()
        .filter_map(|object| {
            scene.read_component::<ComponentTerrain, _, _>(object, |terrain| {
                (object, terrain.size.x, terrain.size.y, terrain.height_scale)
            })
        })
        .filter(|&(object, ..)| scene.is_component_enabled::<ComponentTerrain>(object))
        .filter_map(|(object, width, depth, height_scale)| {
            let (min, max) = terrain_bounds(scene, object, width, depth, height_scale);
            let (from, to) = clip_to_box(start, end, min, max)?;
            let below = |time: f32| {
                let point = start + (end - start) * time;
                ComponentTerrain::sample(scene, object, vec2(point.x, point.z))
                    .is_some_and(|sample| point.y <= sample.height)
            };
            if below(from) {
                return Some(from);
            }
            let steps = (((to - from) * length / TERRAIN_MARCH_STEP).ceil() as usize).max(1);
            let mut above = from;
            for step in 1..=steps {
                let time = from + (to - from) * step as f32 / steps as f32;
                if below(time) {
                    let mut under = time;
                    for _ in 0..TERRAIN_REFINE_STEPS {
                        let middle = (above + under) * 0.5;
                        if below(middle) {
                            under = middle;
                        } else {
                            above = middle;
                        }
                    }
                    return Some(under);
                }
                above = time;
            }
            None
        })
        .min_by(f32::total_cmp)
}

/// The world-space box around everything `terrain`'s surface can reach.
fn terrain_bounds(
    scene: &Scene,
    terrain: GameObject,
    width: f32,
    depth: f32,
    height_scale: f32,
) -> (Vec3, Vec3) {
    let world = scene.world_transform(terrain).matrix();
    let mut min = Vec3::repeat(f32::INFINITY);
    let mut max = Vec3::repeat(f32::NEG_INFINITY);
    for corner in 0..8 {
        let local = vec4(
            if corner & 1 == 0 { -0.5 } else { 0.5 } * width,
            if corner & 2 == 0 { 0.0 } else { height_scale },
            if corner & 4 == 0 { -0.5 } else { 0.5 } * depth,
            1.0,
        );
        let point = (world * local).xyz();
        min = min.inf(&point);
        max = max.sup(&point);
    }
    (min, max)
}

/// The part of the `start`..`end` path inside the box from `min` to `max`,
/// as fractions of the path.
fn clip_to_box(start: Vec3, end: Vec3, min: Vec3, max: Vec3) -> Option<(f32, f32)> {
    let (mut from, mut to) = (0.0f32, 1.0f32);
    let delta = end - start;
    for axis in 0..3 {
        if delta[axis].abs() <= f32::EPSILON {
            if start[axis] < min[axis] || start[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let a = (min[axis] - start[axis]) / delta[axis];
        let b = (max[axis] - start[axis]) / delta[axis];
        from = from.max(a.min(b));
        to = to.min(a.max(b));
    }
    (from <= to).then_some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calyx_engine::assets::AssetRef;
    use calyx_engine::test_support::TestAssets;
    use image::{ImageBuffer, Luma};
    use nalgebra_glm::{vec2, vec3};

    /// A scene with an 8 by 8 m terrain at the origin whose single mound
    /// peaks 2 m high at its centre and falls away linearly to the floor 2 m
    /// out, along each axis. Keep the `TestAssets` alive with the scene.
    fn mound() -> (TestAssets, Scene) {
        // The texture's upload needs the engine's mip shaders.
        let assets = TestAssets::engine();
        let path = assets.engine_root().join("mound.png");
        ImageBuffer::from_fn(5, 5, |x, y| {
            Luma([if (x, y) == (2, 2) { 65535u16 } else { 0 }])
        })
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
                height_scale: 2.0,
                ..Default::default()
            },
        );
        (assets, scene)
    }

    #[test]
    fn the_terrain_is_ground_under_a_point() {
        let (_assets, scene) = mound();
        let none = HashSet::new();
        // Halfway up the mound's side, within reach below the point.
        let height = surface_height_below(&scene, &none, vec3(1.0, 3.0, 0.0), 3.0).unwrap();
        assert!((height - 1.0).abs() < 1e-4, "{height}");
        // Out of reach, or above the point, it is not ground.
        assert!(surface_height_below(&scene, &none, vec3(1.0, 3.0, 0.0), 1.0).is_none());
        assert!(surface_height_below(&scene, &none, vec3(0.0, 1.0, 0.0), 3.0).is_none());
    }

    #[test]
    fn the_cursor_aims_at_the_terrain_surface() {
        let (_assets, scene) = mound();
        // Straight down over the mound's side lands on its surface.
        let ray = Ray {
            origin: vec3(1.0, 30.0, 0.0),
            direction: vec3(0.0, -1.0, 0.0),
        };
        let aimed = aimed_surface_point(&scene, &ray).unwrap();
        assert!(
            (aimed - vec3(1.0, 1.0, 0.0)).magnitude() < 1e-3,
            "{aimed:?}"
        );
        // A slanted ray meets the near side of the mound first: at x = -1.2
        // it is 0.8 m above the floor, where the surface is.
        let ray = Ray {
            origin: vec3(-30.0, 29.6, 0.0),
            direction: vec3(1.0, -1.0, 0.0).normalize(),
        };
        let aimed = aimed_surface_point(&scene, &ray).unwrap();
        assert!(
            (aimed - vec3(-1.2, 0.8, 0.0)).magnitude() < 1e-2,
            "{aimed:?}"
        );
        // Beside the terrain there is no ground.
        let ray = Ray {
            origin: vec3(10.0, 30.0, 0.0),
            direction: vec3(0.0, -1.0, 0.0),
        };
        assert!(aimed_surface_point(&scene, &ray).is_none());
    }

    #[test]
    fn rounds_stop_at_a_rise_and_fly_on_above_it() {
        let (_assets, scene) = mound();
        // A level round 1 m up, of radius 0.2, touches the side where it is
        // 0.8 m high: 1.2 m from the peak, at x = -1.2.
        let (start, end) = (vec3(-4.0, 1.0, 0.0), vec3(4.0, 1.0, 0.0));
        let time = terrain_hit_time(&scene, start, end, 0.2).unwrap();
        let x = start.x + (end.x - start.x) * time;
        assert!((x + 1.2).abs() < 1e-2, "{x}");
        // Above the peak, the round flies on.
        assert!(terrain_hit_time(&scene, vec3(-4.0, 2.3, 0.0), vec3(4.0, 2.3, 0.0), 0.2).is_none());
        // Beside the mound, over level ground, it flies on.
        assert!(terrain_hit_time(&scene, vec3(-4.0, 1.0, 3.0), vec3(4.0, 1.0, 3.0), 0.2).is_none());
        // One step that would jump over the mound's peak still stops there.
        let time =
            terrain_hit_time(&scene, vec3(-3.0, 1.9, 0.0), vec3(3.0, 1.9, 0.0), 0.0).unwrap();
        assert!(time > 0.0 && time < 0.5, "{time}");
    }
}
