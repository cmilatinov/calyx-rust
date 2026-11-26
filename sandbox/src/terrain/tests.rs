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
