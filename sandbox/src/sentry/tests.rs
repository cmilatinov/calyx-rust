use super::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentProjectile, ComponentProjectileTarget};
use calyx_engine::component::{ColliderShape, ComponentCollider, ComponentRigidBody};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::test_support::HeadlessSceneRunner;
use nalgebra_glm::vec3;
use rapier3d::dynamics::RigidBodyType;

/// A sentry at the origin aimed along nothing yet, and a tank `distance`
/// metres down +Z.
fn sentry_and_tank(distance: f32) -> (Scene, GameObject, GameObject) {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = scene.create(None, None);
    scene.set_world_transform(tank, Transform::from_xyz(0.0, 0.0, distance).matrix());
    scene.add_component(
        tank,
        ComponentProjectileTarget {
            hit_radius: 1.25,
            ..Default::default()
        },
    );
    scene.add_component(tank, ComponentHealth::default());
    scene.add_component(tank, ComponentRespawnState::default());

    let sentry = scene.create(None, None);
    scene.set_world_transform(sentry, Transform::from_xyz(0.0, 0.5, 0.0).matrix());
    let target = GameObjectRef::new(scene.uuid(tank));
    scene.add_component(
        sentry,
        ComponentSentry {
            target,
            range: 12.0,
            fire_interval: 1.0,
            projectile_damage: 25.0,
            ..Default::default()
        },
    );
    (scene, sentry, tank)
}

fn health(scene: &Scene, tank: GameObject) -> f32 {
    scene
        .read_component::<ComponentHealth, _, _>(tank, |health| health.current_health)
        .expect("tank should keep its health")
}

fn rounds(scene: &Scene) -> usize {
    scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentProjectile, _, _>(object, |_| ())
                .is_some()
        })
        .count()
}

#[test]
fn sentry_fires_at_a_tank_in_range_and_damages_it() {
    let (scene, sentry, tank) = sentry_and_tank(8.0);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step();
    assert_eq!(rounds(runner.scene()), 1, "the sentry should fire at once");
    let facing = runner.scene().world_transform(sentry).forward();
    assert!(
        facing.z > 0.99,
        "the sentry should turn to the tank: {facing:?}"
    );

    runner.step_many(60);

    assert!(
        health(runner.scene(), tank) < 100.0,
        "a round should reach the tank and damage it"
    );
}

#[test]
fn cover_between_the_sentry_and_the_tank_absorbs_its_rounds() {
    let (mut scene, _, tank) = sentry_and_tank(8.0);
    let cover = scene.create(None, None);
    scene.set_world_transform(cover, Transform::from_xyz(0.0, 0.5, 4.0).matrix());
    scene.add_component(
        cover,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    scene.add_component(
        cover,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(2.0, 1.0, 0.5),
            },
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step_many(120);

    assert_eq!(health(runner.scene(), tank), 100.0);
}

#[test]
fn sentry_fires_across_a_tank_blocker_and_damages_the_tank() {
    let (mut scene, _, tank) = sentry_and_tank(8.0);
    let water = scene.create(None, None);
    scene.set_world_transform(water, Transform::from_xyz(0.0, 0.5, 4.0).matrix());
    scene.add_component(
        water,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    scene.add_component(
        water,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(2.0, 1.0, 0.5),
            },
            groups: crate::collision::TANK_BLOCKER,
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step_many(60);

    assert!(health(runner.scene(), tank) < 100.0);
}

#[test]
fn sentry_rounds_climb_to_a_tank_on_higher_ground() {
    let (mut scene, _, tank) = sentry_and_tank(8.0);
    // The floor under the sentry is a drive surface; the tank stands on
    // a deck 1.6 m above it.
    let floor = scene.create(None, None);
    scene.set_world_transform(floor, Transform::from_xyz(0.0, -0.5, 0.0).matrix());
    scene.add_component(
        floor,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    scene.add_component(
        floor,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: vec3(4.0, 0.5, 4.0),
            },
            ..Default::default()
        },
    );
    scene.add_component(floor, crate::terrain::ComponentDriveSurface {});
    scene.set_world_transform(tank, Transform::from_xyz(0.0, 1.6, 8.0).matrix());
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step();
    let climb = runner
        .scene()
        .objects()
        .find_map(|object| {
            runner
                .scene()
                .read_component::<ComponentProjectile, _, _>(object, |p| p.direction)
        })
        .expect("the sentry should fire");
    assert!(climb.y > 0.15, "the round climbs: {climb:?}");
    runner.step_many(60);
    assert!(health(runner.scene(), tank) < 100.0);
}

#[test]
fn sentry_holds_fire_while_the_tank_is_out_of_range() {
    let (scene, _, tank) = sentry_and_tank(20.0);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step_many(120);

    assert_eq!(rounds(runner.scene()), 0);
    assert_eq!(health(runner.scene(), tank), 100.0);
}

#[test]
fn sentry_waits_its_fire_interval_between_rounds() {
    let (mut scene, sentry, _) = sentry_and_tank(11.0);
    // Slow rounds stay in flight across the interval, so each is counted.
    let _ = scene.write_component::<ComponentSentry, _>(sentry, |sentry| {
        sentry.projectile_speed = 1.0;
    });
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(0.25);

    runner.step();
    runner.step_many(3);
    assert_eq!(
        rounds(runner.scene()),
        1,
        "0.75 s after the first round is inside the interval"
    );

    runner.step();
    assert_eq!(rounds(runner.scene()), 2, "1 s after the first round");
}

#[test]
fn sentry_holds_fire_while_the_tank_is_dead() {
    let (mut scene, _, tank) = sentry_and_tank(8.0);
    let _ = scene.write_component::<ComponentRespawnState, _>(tank, |state| {
        state.alive = false;
        state.respawn_remaining = 10.0;
    });
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.step_many(30);

    assert_eq!(rounds(runner.scene()), 0);
}

#[test]
fn visual_head_tracks_the_target_without_turning_the_base_or_changing_the_shooter() {
    let (mut scene, sentry, tank) = sentry_and_tank(8.0);
    let head = scene.create(None, Some(sentry));
    scene.set_transform(head, &Transform::from_xyz(0.0, -0.2, 0.0).matrix());
    let muzzle = scene.create(None, Some(head));
    scene.set_transform(muzzle, &Transform::from_xyz(0.0, 0.2, 1.2).matrix());
    let head_ref = GameObjectRef::new(scene.uuid(head));
    let _ = scene.write_component::<ComponentSentry, _>(sentry, |component| {
        component.head = head_ref;
    });
    let base = scene.world_transform(sentry);
    scene.set_world_transform(tank, Transform::from_xyz(8.0, 0.0, 0.0).matrix());

    super::update_sentry(&mut scene, sentry, 0.0);

    assert_eq!(scene.world_transform(sentry).matrix(), base.matrix());
    assert!(scene.world_transform(head).forward().x > 0.99);
    let round = scene
        .objects()
        .find(|&object| {
            scene
                .read_component::<ComponentProjectile, _, _>(object, |_| ())
                .is_some()
        })
        .unwrap();
    let projectile = scene
        .read_component::<ComponentProjectile, _, _>(round, |round| *round)
        .unwrap();
    assert_eq!(projectile.shooter.game_object(&scene), Some(sentry));
    let shell_parts = scene
        .children(round)
        .filter_map(|part| {
            scene.read_component::<calyx_engine::component::ComponentMesh, _, _>(part, |mesh| {
                mesh.mesh.clone()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(shell_parts.len(), 2);
    assert!(
        shell_parts.iter().all(|mesh| *mesh
            == calyx_engine::assets::AssetRef::from_id(
                "52c056b4-6dfb-1859-6023-23693f3ff263".parse().unwrap()
            )),
        "Sentry rounds keep the shared shell and luminous tail, not turret meshes"
    );
    assert!(
        (scene.world_transform(round).position - scene.world_transform(muzzle).position).norm()
            < 1e-5
    );

    // A cooldown prevents firing, not aiming. Its model continues tracking.
    scene.set_world_transform(tank, Transform::from_xyz(-8.0, 0.0, 0.0).matrix());
    super::update_sentry(&mut scene, sentry, 0.1);
    assert_eq!(rounds(&scene), 1);
    assert!(scene.world_transform(head).forward().x < -0.99);
    assert_eq!(scene.world_transform(sentry).matrix(), base.matrix());
}
