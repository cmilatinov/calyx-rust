use super::abilities::{kick, ComponentTankAbilities};
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
    scene.add_component(tank, ComponentKnockout::default());
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

fn meter(scene: &Scene, tank: GameObject) -> (f32, f32) {
    scene
        .read_component::<ComponentKnockout, _, _>(tank, |knockout| {
            (knockout.state.percent, knockout.knockback.magnitude())
        })
        .unwrap()
}

#[test]
fn tanks_without_prototype_components_keep_player_control_and_health() {
    let mut scene = calyx_engine::test_support::test_scene();
    let plain = scene.create(None, None);
    scene.add_component(plain, crate::tank::ComponentTankController::default());
    assert_eq!(tank_control(&scene, plain), TankControl::Player);
    assert_eq!(traction(&scene, plain), 1.0);
    // A round on a tank without a meter is left to its health.
    assert!(!absorb_round(&mut scene, plain, 35.0, Vec3::z()));
}

#[test]
fn a_dummy_drives_its_loop_stays_put_or_fights() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = tank(&mut scene, "Player", vec3(0.0, 0.0, 20.0));
    let dummy = tank(&mut scene, "Dummy", Vec3::zeros());
    scene.add_component(
        dummy,
        ComponentDummyDriver {
            mode: DummyMode::DriveLoop,
            ..Default::default()
        },
    );
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
    // The bot heads for the player, and aims at it.
    scene.write_component::<ComponentDummyDriver, _>(dummy, |driver| {
        driver.mode = DummyMode::Bot;
    });
    let TankControl::Bot { throttle, aim, .. } = tank_control(&scene, dummy) else {
        panic!("a bot");
    };
    assert_eq!(throttle, 1.0);
    assert_eq!(aim, scene.world_transform(player).position);
}

#[test]
fn rounds_add_to_the_meter_and_shove_and_a_launch_turns_the_body_dynamic() {
    let mut scene = calyx_engine::test_support::test_scene();
    let target = tank(&mut scene, "Target", vec3(0.0, 0.0, 5.0));
    scene.prepare();
    assert_eq!(
        body_type(&scene, target),
        RigidBodyType::KinematicPositionBased
    );

    assert!(absorb_round(&mut scene, target, 35.0, Vec3::z()));
    let (percent, shove) = meter(&scene, target);
    assert_eq!(percent, 5.0);
    assert!(shove > 0.0, "a weak round only shoves");
    assert_eq!(tank_control(&scene, target), TankControl::Player);

    // A kick at a high meter launches.
    scene.write_component::<ComponentKnockout, _>(target, |knockout| {
        knockout.state.percent = 120.0;
    });
    let abilities = ComponentTankAbilities {
        boot_hitstop: 0.0,
        ..Default::default()
    };
    let reaction = strike(&mut scene, target, kick(&abilities, Vec3::z())).unwrap();
    assert!(matches!(reaction, Reaction::Launch(_)), "{reaction:?}");
    let stance = scene
        .read_component::<ComponentKnockout, _, _>(target, |knockout| knockout.state.stance)
        .unwrap();
    assert_eq!(stance, Stance::KnockedDown { elapsed: 0.0 });
    assert_eq!(body_type(&scene, target), RigidBodyType::Dynamic);
    let velocity = body_velocity(&scene, target);
    // 11 m/s at 0%, grown by 136% of the meter.
    assert!(velocity.z > 11.0 * 2.5, "{velocity:?}");
    assert!(velocity.y > 0.0);
    assert!(matches!(
        tank_control(&scene, target),
        TankControl::Locked { .. }
    ));
    assert_eq!(
        scene
            .read_component::<ComponentRigidBody, _, _>(target, |body| body.ty)
            .unwrap(),
        RigidBodyType::Dynamic
    );
}

#[test]
fn a_kick_hits_far_harder_than_a_round_and_freezes_both_tanks() {
    let mut scene = calyx_engine::test_support::test_scene();
    let shot = tank(&mut scene, "Shot", vec3(0.0, 0.0, 5.0));
    let kicked = tank(&mut scene, "Kicked", vec3(10.0, 0.0, 5.0));
    scene.prepare();
    let abilities = ComponentTankAbilities::default();

    absorb_round(&mut scene, shot, 35.0, Vec3::z());
    let reaction = strike(&mut scene, kicked, kick(&abilities, Vec3::z())).unwrap();
    let (shot_percent, _) = meter(&scene, shot);
    let (kicked_percent, _) = meter(&scene, kicked);
    assert!(kicked_percent > shot_percent * 3.0);
    // Even at 0% a kick launches, but only once the hit-stop ends.
    assert!(matches!(reaction, Reaction::Launch(_)), "{reaction:?}");
    assert!(matches!(
        tank_control(&scene, kicked),
        TankControl::Locked { .. }
    ));
    assert_eq!(
        body_type(&scene, kicked),
        RigidBodyType::KinematicPositionBased,
        "frozen until the hit-stop ends"
    );
}

#[test]
fn an_anchored_tank_takes_the_meter_but_no_knockback() {
    let mut scene = calyx_engine::test_support::test_scene();
    let anchored = tank(&mut scene, "Anchored", Vec3::zeros());
    scene.prepare();
    scene.write_component::<ComponentKnockout, _>(anchored, |knockout| {
        knockout.state.anchored = 1.0;
        knockout.state.percent = 150.0;
    });
    let abilities = ComponentTankAbilities::default();
    let reaction = strike(&mut scene, anchored, kick(&abilities, Vec3::z())).unwrap();
    assert_eq!(reaction, Reaction::Ignored);
    let (percent, shove) = meter(&scene, anchored);
    assert_eq!(percent, 150.0 + abilities.boot_damage);
    assert_eq!(shove, 0.0);
    // Nor does a bumper launch it.
    assert!(!launch(&mut scene, anchored, Vec3::y() * 10.0, 0.01, 8.0));
    assert_eq!(
        body_type(&scene, anchored),
        RigidBodyType::KinematicPositionBased
    );
}

#[test]
fn oil_takes_the_grip_of_tanks_on_it() {
    let mut scene = calyx_engine::test_support::test_scene();
    let oiled = tank(&mut scene, "Oiled", vec3(1.0, 0.0, 1.0));
    let dry = tank(&mut scene, "Dry", vec3(10.0, 0.0, 0.0));
    let oil = scene.create(None, None);
    scene.add_component(
        oil,
        abilities::ComponentOilSlick {
            half_size: 2.0,
            traction: 0.2,
            ..Default::default()
        },
    );
    assert_eq!(traction(&scene, oiled), 0.2);
    assert_eq!(traction(&scene, dry), 1.0);
}

#[test]
fn the_sandbox_binds_every_prototype_ability() {
    let text = include_str!("../../assets/input.cxinput");
    let map: calyx_engine::input::ActionMap = serde_json::from_str(text).unwrap();
    let saved = serde_json::to_value(&map).unwrap();
    for action in [
        abilities::BOOT_ACTION,
        abilities::HOOK_ACTION,
        abilities::BOOST_ACTION,
        abilities::ANCHOR_ACTION,
        abilities::MINE_ACTION,
        abilities::OIL_ACTION,
        abilities::RESET_ACTION,
    ] {
        assert!(
            saved["actions"].get(action).is_some(),
            "{action} is unbound"
        );
    }
}

/// A fixed box collider of `size` centred at `center`, like a wall.
fn wall(scene: &mut Scene, center: Vec3, size: Vec3) -> GameObject {
    let wall = scene.create(None, None);
    scene.set_world_transform(
        wall,
        Transform::from_xyz(center.x, center.y, center.z).matrix(),
    );
    scene.add_component(
        wall,
        ComponentRigidBody {
            ty: RigidBodyType::Fixed,
            ..Default::default()
        },
    );
    scene.add_component(
        wall,
        ComponentCollider {
            shape: ColliderShape::Cuboid {
                half_extents: size * 0.5,
            },
            ..Default::default()
        },
    );
    wall
}

#[test]
fn a_tank_recovering_inside_a_wall_is_moved_clear_towards_the_arena() {
    let mut scene = calyx_engine::test_support::test_scene();
    // A wall 1.2 m thick along x, its faces at z = -15.4 and -16.6.
    wall(&mut scene, vec3(10.0, 0.8, -16.0), vec3(8.0, 1.6, 1.2));
    scene.prepare();
    let lying = Transform::from_components(
        vec3(10.0, 0.0, -16.0),
        UnitQuaternion::from_euler_angles(0.0, 1.3, 0.0),
        vec3(1.0, 1.0, 1.0),
    );
    assert!(!hull_free(&scene, lying.position, lying.rotation));
    let (position, rotation) = recovery_pose(&scene, &lying, 0.0, Vec3::zeros());
    assert!(hull_free(&scene, position, rotation), "{position:?}");
    assert!(position.z > -15.4, "out on the arena's side: {position:?}");
    assert!(
        (position - lying.position).magnitude() < 4.0,
        "{position:?}"
    );
}

#[test]
fn a_tank_recovering_in_the_open_stays_where_it_is() {
    let mut scene = calyx_engine::test_support::test_scene();
    wall(&mut scene, vec3(10.0, 0.8, -16.0), vec3(8.0, 1.6, 1.2));
    scene.prepare();
    let lying = Transform::from_components(
        vec3(10.0, 0.0, -10.0),
        UnitQuaternion::from_euler_angles(0.0, 0.4, 0.0),
        vec3(1.0, 1.0, 1.0),
    );
    let (position, _) = recovery_pose(&scene, &lying, 0.0, Vec3::zeros());
    assert_eq!(position, lying.position);
}

#[test]
fn a_reset_puts_every_tank_back_at_its_spawn_and_clears_the_round() {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = tank(&mut scene, "Tank", vec3(0.0, 0.0, -10.0));
    scene.add_component(tank, ComponentTankAbilities::default());
    scene.prepare();
    let spawn = scene.world_transform(tank);
    scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
        knockout.home = Some(spawn);
        knockout.state.percent = 140.0;
        knockout.state.knockouts = 3;
        knockout.hitstop = 0.05;
    });
    scene.write_component::<ComponentTankAbilities, _>(tank, |abilities| {
        abilities.boot_cooldown_remaining = 1.0;
        abilities.boot_damage = 30.0;
    });
    knock_down(&mut scene, tank, vec3(0.0, 5.0, 10.0), 3.0);
    scene.set_world_transform(tank, Transform::from_xyz(8.0, 2.0, 4.0).matrix());
    let mine = scene.create(None, None);
    scene.add_component(mine, abilities::ComponentBumperMine::default());
    let oil = scene.create(None, None);
    scene.add_component(oil, abilities::ComponentOilSlick::default());

    reset_round(&mut scene);
    scene.flush_deletes();

    let (percent, knockouts, stance, hitstop) = scene
        .read_component::<ComponentKnockout, _, _>(tank, |knockout| {
            (
                knockout.state.percent,
                knockout.state.knockouts,
                knockout.state.stance,
                knockout.hitstop,
            )
        })
        .unwrap();
    assert_eq!(
        (percent, knockouts, stance, hitstop),
        (0.0, 0, Stance::Upright, 0.0)
    );
    assert_eq!(scene.world_transform(tank).position, spawn.position);
    assert_eq!(
        body_type(&scene, tank),
        RigidBodyType::KinematicPositionBased
    );
    let (cooldown, damage) = scene
        .read_component::<ComponentTankAbilities, _, _>(tank, |abilities| {
            (abilities.boot_cooldown_remaining, abilities.boot_damage)
        })
        .unwrap();
    assert_eq!(cooldown, 0.0);
    assert_eq!(damage, 30.0, "the tuning stays");
    assert!(scene
        .objects_with::<abilities::ComponentBumperMine>()
        .next()
        .is_none());
    assert!(scene
        .objects_with::<abilities::ComponentOilSlick>()
        .next()
        .is_none());
}

#[test]
fn a_reset_does_nothing_without_prototype_tanks() {
    let mut scene = calyx_engine::test_support::test_scene();
    let round = scene.create(None, None);
    scene.add_component(round, crate::tank::ComponentProjectile::default());
    reset_round(&mut scene);
    scene.flush_deletes();
    assert!(scene
        .objects_with::<crate::tank::ComponentProjectile>()
        .next()
        .is_some());
}
