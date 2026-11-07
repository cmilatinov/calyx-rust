//! Drive surfaces: the floor, ramps and platform decks tanks ride on and
//! the cursor aims at. Everything else with a static collider is an
//! obstacle that stops the hull.

use calyx_engine::component::{Component, ComponentCollider, ReflectComponent};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::Ray;
use calyx_engine::scene::Scene;
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::Entity;
use nalgebra_glm::{vec3, Vec3};
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

/// Height of the highest drive surface under `point` within `below` metres
/// of it, looking down from `point` itself.
pub(crate) fn surface_height_below(
    scene: &Scene,
    surfaces: &HashSet<Entity>,
    point: Vec3,
    below: f32,
) -> Option<f32> {
    first_surface_hit(scene, surfaces, point, vec3(0.0, -1.0, 0.0), below).map(|hit| hit.y)
}

/// Where `ray`, such as the cursor's, first meets a drive surface.
pub(crate) fn aimed_surface_point(scene: &Scene, ray: &Ray) -> Option<Vec3> {
    first_surface_hit(
        scene,
        &drive_surfaces(scene),
        ray.origin,
        ray.direction,
        1000.0,
    )
}
