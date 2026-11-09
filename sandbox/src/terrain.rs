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
        .objects_with::<ComponentDriveSurface>()
        .filter(|&object| {
            scene
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

#[cfg(test)]
mod tests {
    use super::{aimed_surface_point, ComponentDriveSurface};
    use calyx_engine::component::{
        ColliderShape, CollisionGroups, ComponentCollider, ComponentRigidBody,
    };
    use calyx_engine::math::Transform;
    use calyx_engine::render::Ray;
    use calyx_engine::scene::Scene;
    use nalgebra_glm::{vec3, Vec3};
    use rapier3d::dynamics::RigidBodyType;

    /// A fixed box collider in `groups`; a drive surface when `drive_surface`.
    fn fixed_box(
        scene: &mut Scene,
        center: Vec3,
        half_extents: Vec3,
        groups: CollisionGroups,
        drive_surface: bool,
    ) {
        let object = scene.create(None, None);
        scene.set_world_transform(
            object,
            Transform::from_xyz(center.x, center.y, center.z).matrix(),
        );
        scene.add_component(
            object,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            object,
            ComponentCollider {
                shape: ColliderShape::Cuboid { half_extents },
                groups,
                ..Default::default()
            },
        );
        if drive_surface {
            scene.add_component(object, ComponentDriveSurface {});
        }
    }

    #[test]
    fn the_aim_reaches_the_floor_across_a_tank_blocker() {
        let mut scene = calyx_engine::test_support::test_scene();
        // The floor's top is at y = 0; a tank-only blocker stands on it.
        fixed_box(
            &mut scene,
            vec3(0.0, -0.5, 0.0),
            vec3(20.0, 0.5, 20.0),
            CollisionGroups::ALL,
            true,
        );
        fixed_box(
            &mut scene,
            vec3(0.0, 1.0, 3.0),
            vec3(5.0, 1.0, 1.0),
            crate::collision::TANK_BLOCKER,
            false,
        );
        scene.prepare();

        // A cursor ray from above the tank that passes through the blocker
        // on its way to the floor beyond it.
        let origin = vec3(0.0, 3.0, 0.0);
        let ray = Ray {
            origin,
            direction: (vec3(0.0, 0.0, 6.0) - origin).normalize(),
        };
        let aim = aimed_surface_point(&scene, &ray).expect("the aim reaches the floor");
        assert!((aim - vec3(0.0, 0.0, 6.0)).norm() < 1e-3, "aim: {aim:?}");
    }
}
