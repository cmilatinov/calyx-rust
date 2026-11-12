use super::logic::Stance;
use super::*;
use calyx_engine::component::ComponentID;
use calyx_engine::scene::Scene;

/// A prototype tank: kinematic, with the prefab's colliders on children.
fn tank(scene: &mut Scene, name: &str, position: Vec3) -> GameObject {
    let tank = scene.create(
        Some(ComponentID {
            name: name.into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(
        tank,
        Transform::from_xyz(position.x, position.y, position.z).matrix(),
    );
    scene.add_component(tank, crate::tank::ComponentTankController::default());
    scene.add_component(tank, ComponentPoise::default());
    scene.add_component(
        tank,
        ComponentRigidBody {
            ty: RigidBodyType::KinematicPositionBased,
            can_sleep: false,
            ..Default::default()
        },
    );
    for (name, center, half_extents) in TANK_COLLIDERS {
        let collider = scene.create(
            Some(ComponentID {
                name: name.into(),
                ..Default::default()
            }),
            Some(tank),
        );
        scene.set_world_transform(
            collider,
            Transform::from_xyz(
                position.x + center.x,
                position.y + center.y,
                position.z + center.z,
            )
            .matrix(),
        );
        scene.add_component(collider, tank_collider(half_extents));
    }
    tank
}

fn body_type(scene: &Scene, tank: GameObject) -> RigidBodyType {
    scene.physics.bodies[body_handle(scene, tank).unwrap()].body_type()
}

#[test]
fn tanks_without_prototype_components_keep_player_control_and_health() {
    let mut scene = calyx_engine::test_support::test_scene();
    let plain = scene.create(None, None);
    scene.add_component(plain, crate::tank::ComponentTankController::default());
    assert_eq!(tank_control(&scene, plain), TankControl::Player);
    // A round on a tank without poise is left to its health.
    assert!(!absorb_round(&mut scene, plain, 35.0, Vec3::z()));
}

#[test]
fn a_dummy_drives_its_loop_or_stays_put() {
    let mut scene = calyx_engine::test_support::test_scene();
    let dummy = tank(&mut scene, "Dummy", Vec3::zeros());
    scene.add_component(dummy, ComponentDummyDriver::default());
    assert_eq!(
        tank_control(&scene, dummy),
        TankControl::Dummy {
            throttle: 0.6,
            steer: 0.35
        }
    );
    scene.write_component::<ComponentDummyDriver, _>(dummy, |driver| {
        driver.mode = DummyMode::StayPut;
    });
    assert_eq!(
        tank_control(&scene, dummy),
        TankControl::Dummy {
            throttle: 0.0,
            steer: 0.0
        }
    );
}

#[test]
fn rounds_drain_poise_then_a_break_turns_the_body_dynamic_and_locks_input() {
    let mut scene = calyx_engine::test_support::test_scene();
    let target = tank(&mut scene, "Target", vec3(0.0, 0.0, 5.0));
    scene.prepare();
    assert_eq!(
        body_type(&scene, target),
        RigidBodyType::KinematicPositionBased
    );

    assert!(absorb_round(&mut scene, target, 35.0, Vec3::z()));
    let (poise, knockback) = scene
        .read_component::<ComponentPoise, _, _>(target, |poise| {
            (poise.state.poise, poise.knockback)
        })
        .unwrap();
    assert_eq!(poise, 65.0);
    assert!(knockback.z > 0.0, "a round that holds still shoves");
    assert_eq!(tank_control(&scene, target), TankControl::Player);

    absorb_round(&mut scene, target, 35.0, Vec3::z());
    absorb_round(&mut scene, target, 35.0, Vec3::z());
    let stance = scene
        .read_component::<ComponentPoise, _, _>(target, |poise| poise.state.stance)
        .unwrap();
    assert_eq!(stance, Stance::KnockedDown { elapsed: 0.0 });
    assert_eq!(body_type(&scene, target), RigidBodyType::Dynamic);
    let velocity = body_velocity(&scene, target);
    assert!(velocity.z > 0.0 && velocity.y > 0.0, "{velocity:?}");
    assert!(matches!(
        tank_control(&scene, target),
        TankControl::Locked { .. }
    ));
    // The component agrees, so the next physics sync keeps it dynamic.
    assert_eq!(
        scene
            .read_component::<ComponentRigidBody, _, _>(target, |body| body.ty)
            .unwrap(),
        RigidBodyType::Dynamic
    );
}
