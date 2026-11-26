mod health_trail;

use super::{
    advance_fire_cooldown, advance_reload, advance_speed, apply_projectile_damage, can_fire,
    flatten_xz, follow_camera_xz, is_reloading, resolve_hull_movement, segment_intersects_sphere,
    spawn_projectile, turn_hull, turn_towards, update_projectile, yaw_rotation, ComponentHealth,
    ComponentProjectile, ComponentProjectileTarget, ComponentTankController,
};
use crate::collision::TANK_BLOCKER;
use crate::combat::{ComponentCombatStats, ComponentKillFeed};
use crate::spawn::{update_respawn_state, ComponentRespawnState, ComponentSpawnPoint};
use crate::tank_camera::ground_intersection;
use calyx_engine::component::{
    ColliderShape, CollisionGroups, ComponentCollider, ComponentID, ComponentRigidBody,
};
use calyx_engine::math::Transform;
use calyx_engine::render::Camera;
use calyx_engine::scene::GameObjectRef;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3};
use rapier3d::dynamics::RigidBodyType;

#[test]
fn player_prefab_instances_remap_their_reticles_and_move_them_independently() {
    use calyx_engine::component::ComponentMesh;
    use calyx_engine::scene::{Prefab, PrefabData};

    let prefab: Prefab =
        serde_json::from_str::<PrefabData>(include_str!("../../assets/game/prefabs/tank.cxprefab"))
            .unwrap()
            .into();
    let mut scene = calyx_engine::test_support::test_scene();
    let mut crosshairs = Vec::new();
    for _ in 0..2 {
        let (_, dropped) = scene.instantiate_prefab_reporting(&prefab, None).unwrap();
        assert!(dropped.is_empty(), "{dropped:?}");
        let controller = scene
            .objects()
            .filter_map(|object| {
                scene.read_component::<ComponentTankController, _, _>(object, |c| *c)
            })
            .find(|controller| !crosshairs.contains(&controller.crosshair.id()))
            .unwrap();
        let crosshair = controller.crosshair.game_object(&scene).unwrap();
        assert!(!prefab.data.components.contains_key(&scene.uuid(crosshair)));
        let parts = scene.children(crosshair).collect::<Vec<_>>();
        assert_eq!(parts.len(), 10);
        for part in parts {
            assert!(!prefab.data.components.contains_key(&scene.uuid(part)));
            assert!(scene
                .read_component::<ComponentMesh, _, _>(part, |_| ())
                .is_some());
        }
        let aim = vec3(3.0 + crosshairs.len() as f32 * 5.0, 0.0, 9.0);
        super::update_crosshair(&mut scene, &controller, Some(aim));
        assert!((scene.world_transform(crosshair).position - aim).norm() < 1e-5);
        crosshairs.push(controller.crosshair.id());
    }
    let first = scene.find(crosshairs[0]).unwrap();
    assert!((scene.world_transform(first).position - vec3(3.0, 0.0, 9.0)).norm() < 1e-5);
}

#[test]
fn yaw_rotation_faces_positive_x() {
    let rotation = yaw_rotation(&vec3(1.0, 0.0, 0.0));
    let forward = rotation * vec3(0.0, 0.0, 1.0);
    assert!((forward.x - 1.0).abs() < 1e-4);
    assert!(forward.z.abs() < 1e-4);
}

#[test]
fn flatten_xz_discards_vertical_component() {
    let flattened = flatten_xz(vec3(3.0, 9.0, 4.0));
    assert!(flattened.y.abs() < 1e-6);
    assert!((flattened.x - 0.6).abs() < 1e-4);
    assert!((flattened.z - 0.8).abs() < 1e-4);
}

#[test]
fn top_down_camera_center_ray_intersects_ground() {
    let tank_transform = Transform::from_xyz(0.0, 0.5, 0.0);
    let camera_transform = Transform::from_components(
        vec3(0.0, 16.0, -10.0),
        UnitQuaternion::from_euler_angles(std::f32::consts::FRAC_PI_3, 0.0, 0.0),
        vec3(1.0, 1.0, 1.0),
    );
    let camera_forward = camera_transform.forward();

    assert!(camera_forward.y < 0.0);
    assert!(camera_forward.z > 0.0);

    let viewport = vec2(1280.0, 720.0);
    let camera = Camera::new(viewport.x / viewport.y, 70.0f32.to_radians(), 0.1, 1000.0);
    let ray = camera
        .screen_ray(&camera_transform, viewport * 0.5, viewport)
        .expect("center ray should exist");
    let hit = ground_intersection(&ray, tank_transform.position.y)
        .expect("center ray should intersect the tank ground plane");

    assert!((hit.x - tank_transform.position.x).abs() < 1e-3);
    assert!((hit.y - tank_transform.position.y).abs() < 1e-3);
}

#[test]
fn sandbox_scene_deserializes_objects() {
    let test_assets = calyx_engine::test_support::TestAssets::sandbox();
    let assets = test_assets.asset_context();
    let scene_ref = assets
        .registries
        .assets
        .read()
        .reload_by_path::<calyx_engine::scene::Scene>(
            &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
        )
        .expect("sandbox scene should load");

    let scene = scene_ref.read();
    let object_count = scene.objects().count();
    let named_object_count = scene
        .objects()
        .filter(|game_object| !scene.name(*game_object).is_empty())
        .count();

    assert!(
        object_count > 0,
        "sandbox scene should deserialize graph objects"
    );
    assert!(
        named_object_count > 0,
        "sandbox scene should deserialize ComponentID data"
    );

    let mut game = calyx_engine::context::GameContext::new(assets);
    game.scenes.load_scene(scene_ref.readonly());
    assert!(
        game.scenes.current_scene().objects().count() > 0,
        "scene manager should install deserialized scene objects"
    );
}

#[test]
fn editor_simulation_start_preserves_sandbox_camera_transform() {
    let test_assets = calyx_engine::test_support::TestAssets::sandbox();
    let assets = test_assets.asset_context();
    let scene_ref = assets
        .registries
        .assets
        .read()
        .reload_by_path::<calyx_engine::scene::Scene>(
            &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
        )
        .expect("sandbox scene should load");

    let mut game = calyx_engine::context::GameContext::new(assets);
    game.scenes.load_scene(scene_ref.readonly());
    let authoring_camera_transform = {
        let scene = game.scenes.current_scene();
        let (camera, _) = scene
            .main_camera()
            .expect("sandbox scene should have a main camera");
        scene.world_transform(camera)
    };
    let authoring_tank_scale = {
        let scene = game.scenes.current_scene();
        let tank = scene
            .objects()
            .find(|go| scene.name(*go) == "Tank")
            .expect("sandbox scene should have a Tank");
        scene.transform(tank).scale
    };

    game.scenes.start_simulation();

    let simulation_camera_transform = {
        let scene = game.scenes.simulation_scene();
        let (camera, _) = scene
            .main_camera()
            .expect("simulation scene should have a main camera");
        scene.world_transform(camera)
    };

    assert_eq!(
        simulation_camera_transform.position,
        authoring_camera_transform.position
    );
    assert_eq!(
        simulation_camera_transform.rotation,
        authoring_camera_transform.rotation
    );
    let simulation_tank_scale = {
        let scene = game.scenes.simulation_scene();
        let tank = scene
            .objects()
            .find(|go| scene.name(*go) == "Tank")
            .expect("simulation scene should have a Tank");
        scene.transform(tank).scale
    };
    assert_eq!(simulation_tank_scale, authoring_tank_scale);
}

#[test]
fn editor_simulation_first_update_preserves_camera_without_input() {
    let test_assets = calyx_engine::test_support::TestAssets::sandbox();
    let assets = test_assets.asset_context();
    let scene_ref = assets
        .registries
        .assets
        .read()
        .reload_by_path::<calyx_engine::scene::Scene>(
            &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
        )
        .expect("sandbox scene should load");

    let mut game = calyx_engine::context::GameContext::new(assets);
    game.scenes.load_scene(scene_ref.readonly());
    let authoring_object_count = game.scenes.current_scene().objects().count();
    game.scenes.start_simulation();
    assert_eq!(
        game.scenes.simulation_scene().objects().count(),
        authoring_object_count
    );
    let initial_camera_transform = {
        let scene = game.scenes.simulation_scene();
        let (camera, _) = scene
            .main_camera()
            .expect("simulation scene should have a main camera");
        scene.world_transform(camera)
    };
    let initial_tank_scale = {
        let scene = game.scenes.simulation_scene();
        let tank = scene
            .objects()
            .find(|go| scene.name(*go) == "Tank")
            .expect("simulation scene should have a Tank");
        scene.transform(tank).scale
    };

    let ctx = egui::Context::default();
    let input = calyx_engine::input::Input::from_ctx(&ctx, None, game.resources.input_mut(), false);
    let registries = game.assets.lock_read().registries;
    game.scenes.update(&registries, &mut game.resources, &input);

    let updated_camera_transform = {
        let scene = game.scenes.simulation_scene();
        let (camera, _) = scene
            .main_camera()
            .expect("simulation scene should have a main camera");
        scene.world_transform(camera)
    };

    assert_eq!(
        updated_camera_transform.position,
        initial_camera_transform.position
    );

    let updated_tank_scale = {
        let scene = game.scenes.simulation_scene();
        let tank = scene
            .objects()
            .find(|go| scene.name(*go) == "Tank")
            .expect("simulation scene should have a Tank");
        scene.transform(tank).scale
    };

    assert_eq!(updated_tank_scale, initial_tank_scale);
}

#[test]
fn sandbox_shooting_does_not_move_targets() {
    let test_assets = calyx_engine::test_support::TestAssets::sandbox();
    let assets = test_assets.asset_context();
    let scene_ref = assets
        .registries
        .assets
        .read()
        .reload_by_path::<calyx_engine::scene::Scene>(
            &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
        )
        .expect("sandbox scene should load");
    let scene = scene_ref.read().clone();
    let target_transforms = ["Target A", "Target B"].map(|name| {
        let target = scene
            .objects()
            .find(|go| scene.name(*go) == name)
            .expect("sandbox scene should have target");
        (name, scene.world_transform(target))
    });

    let mut runner = calyx_engine::test_support::HeadlessSceneRunner::from_scene(scene);
    runner.press_key(egui::Key::Space);
    runner.step_many(120);

    for (name, transform) in target_transforms {
        let scene = runner.scene();
        let target = scene
            .objects()
            .find(|go| scene.name(*go) == name)
            .expect("sandbox scene should still have target");
        let updated = scene.world_transform(target);
        assert_eq!(updated.position, transform.position);
        assert_eq!(updated.rotation, transform.rotation);
        assert_eq!(updated.scale, transform.scale);
    }
}

#[test]
fn camera_follow_only_moves_xz_axes() {
    let rotation = UnitQuaternion::from_euler_angles(std::f32::consts::FRAC_PI_2, 0.2, 0.0);
    let mut camera_transform =
        Transform::from_components(vec3(2.0, 16.0, -8.0), rotation, vec3(1.0, 1.0, 1.0));
    let previous_tank_transform = Transform::from_xyz(1.0, 0.5, 2.0);
    let tank_transform = Transform::from_xyz(4.0, 5.0, 7.0);

    follow_camera_xz(
        &mut camera_transform,
        &previous_tank_transform,
        &tank_transform,
    );

    assert!((camera_transform.position.x - 5.0).abs() < 1e-6);
    assert!((camera_transform.position.y - 16.0).abs() < 1e-6);
    assert!((camera_transform.position.z - -3.0).abs() < 1e-6);
    assert_eq!(camera_transform.rotation, rotation);
}

#[test]
fn hull_movement_stops_before_middle_cover() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_cube(&mut scene, vec3(0.0, 1.0, 3.0), vec3(1.0, 1.0, 1.0));
    scene.prepare();

    let movement = resolve_hull_movement(
        &scene,
        &Transform::from_xyz(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 4.0),
    );

    assert!(movement.z > 0.9);
    assert!(movement.z < 1.0);
}

#[test]
fn hull_cannot_drive_into_a_tank_blocker() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_box(
        &mut scene,
        vec3(0.0, 1.0, 3.0),
        vec3(4.0, 1.0, 1.0),
        TANK_BLOCKER,
    );
    scene.prepare();

    let movement = resolve_hull_movement(
        &scene,
        &Transform::from_xyz(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 4.0),
    );
    assert!(movement.z > 0.9, "movement: {movement:?}");
    assert!(movement.z < 1.0, "the blocker stops the hull: {movement:?}");

    // Nor can the hull turn its corners into it.
    let mut transform = Transform::from_xyz(0.0, 0.0, 0.95);
    assert!(!turn_hull(
        &scene,
        &mut transform,
        std::f32::consts::FRAC_PI_4
    ));
}

#[test]
fn diagonal_hull_movement_slides_along_wall() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_cube(&mut scene, vec3(2.0, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
    scene.prepare();

    let movement = resolve_hull_movement(
        &scene,
        &Transform::from_xyz(0.0, 0.0, 0.0),
        vec3(3.0, 0.0, 2.0),
    );

    assert!(movement.x > 0.7);
    assert!(movement.x < 0.75);
    assert!(movement.z > 1.9);
}

#[test]
fn yawed_hull_slides_along_wall_using_world_normal() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_cube(&mut scene, vec3(2.0, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
    scene.prepare();
    let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);
    transform.rotation = UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0);

    let movement = resolve_hull_movement(&scene, &transform, vec3(3.0, 0.0, 2.0));

    assert!(movement.x > 0.45, "movement: {movement:?}");
    assert!(movement.x < 0.5, "movement: {movement:?}");
    assert!(movement.z > 1.9, "movement: {movement:?}");
}

#[test]
fn hull_movement_ignores_projectile_colliders() {
    let mut scene = calyx_engine::test_support::test_scene();
    let projectile = scene.create(None, None);
    scene.set_transform(projectile, &Transform::from_xyz(0.0, 0.75, 2.0).matrix());
    scene.add_component(
        projectile,
        ComponentRigidBody {
            gravity_scale: 0.0,
            ..Default::default()
        },
    );
    scene.add_component(
        projectile,
        ComponentCollider {
            shape: ColliderShape::Sphere { radius: 0.18 },
            ..Default::default()
        },
    );
    scene.prepare();

    let movement = resolve_hull_movement(
        &scene,
        &Transform::from_xyz(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 4.0),
    );

    assert!((movement.z - 4.0).abs() < 1e-4, "movement: {movement:?}");
}

#[test]
fn hull_does_not_turn_its_corners_into_a_wall() {
    let mut scene = calyx_engine::test_support::test_scene();
    // The wall face sits at x = 0.8, just clear of the hull's 0.75 half
    // width; a quarter turn would swing the 1.0 half length into it.
    add_fixed_cube(&mut scene, vec3(1.3, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
    scene.prepare();
    let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);

    assert!(!turn_hull(
        &scene,
        &mut transform,
        std::f32::consts::FRAC_PI_2
    ));
    assert_eq!(transform.rotation, UnitQuaternion::identity());

    let mut open_transform = Transform::from_xyz(-5.0, 0.0, 0.0);
    assert!(turn_hull(
        &scene,
        &mut open_transform,
        std::f32::consts::FRAC_PI_2
    ));
}

#[test]
fn throttle_accelerates_to_top_speed_and_coasting_stops() {
    let controller = ComponentTankController::default();
    let to_top = controller.move_speed / controller.acceleration;

    let (speed, distance) = advance_speed(0.0, 1.0, &controller, to_top * 0.5);
    assert!((speed - controller.move_speed * 0.5).abs() < 1e-4);
    assert!((distance - controller.move_speed * 0.5 * to_top * 0.25).abs() < 1e-4);

    let (speed, distance) = advance_speed(0.0, 1.0, &controller, to_top + 1.0);
    assert_eq!(speed, controller.move_speed);
    assert!((distance - (controller.move_speed * (to_top * 0.5 + 1.0))).abs() < 1e-3);

    let (speed, _) = advance_speed(0.0, -1.0, &controller, 10.0);
    assert_eq!(speed, -controller.reverse_speed);

    let to_rest = controller.move_speed / controller.friction;
    let (speed, distance) = advance_speed(controller.move_speed, 0.0, &controller, 10.0);
    assert_eq!(speed, 0.0);
    assert!((distance - controller.move_speed * to_rest * 0.5).abs() < 1e-3);
}

#[test]
fn reverse_throttle_brakes_then_reverses() {
    let controller = ComponentTankController::default();
    let to_rest = controller.move_speed / controller.braking;

    let (speed, distance) = advance_speed(controller.move_speed, -1.0, &controller, to_rest);
    assert!(speed.abs() < 1e-4);
    assert!((distance - controller.move_speed * to_rest * 0.5).abs() < 1e-4);

    let (speed, _) = advance_speed(controller.move_speed, -1.0, &controller, to_rest + 0.1);
    assert!((speed + controller.acceleration * 0.1).abs() < 1e-3);
}

#[test]
fn speed_does_not_depend_on_how_time_is_split() {
    let controller = ComponentTankController::default();
    let inputs = [(1.0, 0.8), (-1.0, 0.7), (0.0, 1.0), (-1.0, 0.4), (1.0, 0.6)];
    let drive = |frames_per_second: usize| {
        let dt = 1.0 / frames_per_second as f32;
        let mut speed = 0.0;
        let mut distance = 0.0;
        for (throttle, duration) in inputs {
            for _ in 0..(duration * frames_per_second as f32).round() as usize {
                let (next, step) = advance_speed(speed, throttle, &controller, dt);
                speed = next;
                distance += step;
            }
        }
        (speed, distance)
    };

    let (slow_speed, slow_distance) = drive(30);
    let (fast_speed, fast_distance) = drive(240);
    assert!((slow_speed - fast_speed).abs() < 1e-3);
    assert!((slow_distance - fast_distance).abs() < 1e-3);
}

#[test]
fn turret_turns_the_short_way_at_most_its_step() {
    use std::f32::consts::PI;
    assert!((turn_towards(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
    assert!((turn_towards(0.0, 0.1, 0.25) - 0.1).abs() < 1e-6);
    // From just short of +pi to just past -pi is a small turn through pi.
    let turned = turn_towards(PI - 0.1, -PI + 0.1, 0.05);
    assert!((turned - (PI - 0.05)).abs() < 1e-5);
}

fn add_fixed_cube(
    scene: &mut calyx_engine::scene::Scene,
    position: nalgebra_glm::Vec3,
    half_extents: nalgebra_glm::Vec3,
) {
    add_fixed_box(scene, position, half_extents, CollisionGroups::ALL);
}

/// A fixed box collider in `groups`.
fn add_fixed_box(
    scene: &mut calyx_engine::scene::Scene,
    position: nalgebra_glm::Vec3,
    half_extents: nalgebra_glm::Vec3,
    groups: CollisionGroups,
) -> calyx_engine::scene::GameObject {
    let object = scene.create(None, None);
    scene.set_transform(
        object,
        &Transform::from_components(position, Default::default(), vec3(1.0, 1.0, 1.0)).matrix(),
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
    object
}

#[test]
fn fire_cooldown_counts_down_to_zero() {
    assert!((advance_fire_cooldown(0.35, 0.1) - 0.25).abs() < 1e-6);
    assert_eq!(advance_fire_cooldown(0.1, 0.35), 0.0);
}

#[test]
fn fire_gate_requires_input_and_expired_cooldown() {
    assert!(can_fire(true, 0.0, 1));
    assert!(!can_fire(false, 0.0, 1));
    assert!(!can_fire(true, 0.1, 1));
    assert!(!can_fire(true, 0.0, 0));
}

#[test]
fn reload_refills_empty_ammo_after_duration() {
    let mut controller = ComponentTankController {
        max_ammo: 3,
        ammo: 0,
        reload_duration: 1.0,
        reload_remaining: 1.0,
        ..Default::default()
    };

    advance_reload(&mut controller, 0.4);
    assert_eq!(controller.ammo, 0);
    assert!(is_reloading(&controller));
    assert!((controller.reload_remaining - 0.6).abs() < 1e-6);

    advance_reload(&mut controller, 0.6);
    assert_eq!(controller.ammo, 3);
    assert!(!is_reloading(&controller));
    assert_eq!(controller.reload_remaining, 0.0);
}

#[test]
fn reload_is_disabled_when_max_ammo_is_zero() {
    let mut controller = ComponentTankController {
        max_ammo: 0,
        ammo: 0,
        reload_remaining: 1.0,
        ..Default::default()
    };

    advance_reload(&mut controller, 1.0);

    assert_eq!(controller.ammo, 0);
    assert_eq!(controller.reload_remaining, 0.0);
}

#[test]
fn reload_clamps_stale_ammo_to_max_ammo() {
    let mut controller = ComponentTankController {
        max_ammo: 3,
        ammo: 6,
        reload_remaining: 1.0,
        ..Default::default()
    };

    advance_reload(&mut controller, 0.1);

    assert_eq!(controller.ammo, 3);
    assert_eq!(controller.reload_remaining, 0.0);
}

#[test]
fn projectile_segment_hits_sphere_between_frames() {
    assert!(segment_intersects_sphere(
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 10.0),
        vec3(0.0, 0.0, 5.0),
        0.5,
    ));
    assert!(!segment_intersects_sphere(
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 10.0),
        vec3(2.0, 0.0, 5.0),
        0.5,
    ));
}

#[test]
fn spawned_projectile_has_velocity_and_physics_components() {
    let mut scene = calyx_engine::test_support::test_scene();
    let barrel = scene.create(
        Some(ComponentID {
            name: "Barrel".to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(
        barrel,
        Transform::from_components(
            vec3(0.0, 0.0, 0.0),
            UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
        .matrix(),
    );
    let controller = ComponentTankController {
        barrel: GameObjectRef::new(scene.uuid(barrel)),
        projectile_radius: 0.25,
        projectile_speed: 12.0,
        projectile_damage: 17.0,
        projectile_lifetime: 1.5,
        ..Default::default()
    };

    assert!(spawn_projectile(
        &mut scene,
        barrel,
        &controller,
        &Transform::default(),
        None
    ));

    let projectile = scene
        .objects()
        .find(|object| scene.name(*object) == "Projectile")
        .expect("spawn should create a projectile object");
    let projectile_component = scene
        .read_component::<ComponentProjectile, _, _>(projectile, |component| *component)
        .expect("projectile should have velocity/lifetime data");
    assert_eq!(projectile_component.speed, 12.0);
    assert_eq!(projectile_component.damage, 17.0);
    assert_eq!(projectile_component.lifetime_remaining, 1.5);
    assert_eq!(projectile_component.radius, 0.25);
    assert!((projectile_component.direction.x - 1.0).abs() < 1e-5);
    assert!(projectile_component.direction.z.abs() < 1e-5);
    assert!(
        scene
            .world_transform(projectile)
            .rotation
            .angle_to(&scene.world_transform(barrel).rotation)
            < 1e-5
    );

    let rigid_body = scene
        .read_component::<ComponentRigidBody, _, _>(projectile, |component| {
            (component.gravity_scale, component.can_sleep)
        })
        .expect("projectile should have a rigid body");
    assert_eq!(rigid_body.0, 0.0);
    assert!(!rigid_body.1);

    let collider = scene
        .read_component::<ComponentCollider, _, _>(projectile, |component| component.shape)
        .expect("projectile should have a collider");
    let ColliderShape::Sphere { radius } = collider else {
        panic!("projectile collider should be a sphere");
    };
    assert_eq!(radius, controller.projectile_radius);

    let expected_rotation = scene.world_transform(barrel).rotation;
    scene.prepare();
    let mut runner = calyx_engine::test_support::HeadlessSceneRunner::from_scene(scene);
    for step in 1..=10 {
        runner.step();
        let projectile = runner
            .scene()
            .objects()
            .find(|object| runner.scene().name(*object) == "Projectile")
            .expect("projectile should survive the first updates");
        let actual_rotation = runner.scene().world_transform(projectile).rotation;
        let angle = actual_rotation.angle_to(&expected_rotation);
        assert!(
            angle < 1e-5,
            "projectile rotation changed by {angle} radians on step {step}: {actual_rotation:?}"
        );
    }
}

#[test]
fn projectile_hit_applies_damage_to_health() {
    let mut scene = calyx_engine::test_support::test_scene();
    let projectile = scene.create(
        Some(ComponentID {
            name: "Projectile".to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );

    let target = scene.create(
        Some(ComponentID {
            name: "Target".to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 60.0,
            max_health: 60.0,
            ..Default::default()
        },
    );

    update_projectile(&mut scene, projectile, 0.5);

    let health = scene
        .read_component::<ComponentHealth, _, _>(target, |health| *health)
        .expect("target should keep health after nonlethal hit");
    assert_eq!(health.current_health, 35.0);
    assert!(!health.dead);
    let hit_count = scene
        .read_component::<ComponentProjectileTarget, _, _>(target, |target| target.hit_count)
        .unwrap();
    assert_eq!(hit_count, 0, "health damage replaces legacy hit counting");
}

#[test]
fn arena_wall_stops_projectile_before_target() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_cube(&mut scene, vec3(0.0, 0.0, 2.5), vec3(5.0, 1.0, 0.25));
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );
    let target = scene.create(None, None);
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 60.0,
            max_health: 60.0,
            ..Default::default()
        },
    );
    scene.prepare();

    update_projectile(&mut scene, projectile, 0.5);
    scene.flush_deletes();

    let health = scene
        .read_component::<ComponentHealth, _, _>(target, |health| health.current_health)
        .expect("target should keep its health");
    assert_eq!(health, 60.0, "the wall should absorb the round");
    assert!(
        !scene.objects().any(|object| object == projectile),
        "the round should be destroyed on the wall"
    );
}

#[test]
fn round_crosses_brush_and_hits_the_target_in_it() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_box(
        &mut scene,
        vec3(0.0, 0.75, 4.0),
        vec3(3.0, 0.75, 3.0),
        crate::collision::BRUSH,
    );
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );
    let target = scene.create(None, None);
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 60.0,
            max_health: 60.0,
            ..Default::default()
        },
    );
    scene.prepare();

    update_projectile(&mut scene, projectile, 0.2);
    scene.flush_deletes();
    assert!(
        scene.objects().any(|object| object == projectile),
        "brush lets the round in"
    );
    update_projectile(&mut scene, projectile, 0.5);
    scene.flush_deletes();
    let health = scene
        .read_component::<ComponentHealth, _, _>(target, |health| health.current_health)
        .unwrap();
    assert_eq!(health, 35.0, "the round hits the tank in the brush");
}

#[test]
fn hull_drives_through_brush() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_box(
        &mut scene,
        vec3(0.0, 0.75, 3.0),
        vec3(2.0, 0.75, 1.0),
        crate::collision::BRUSH,
    );
    scene.prepare();
    let movement = resolve_hull_movement(
        &scene,
        &Transform::from_xyz(0.0, 0.0, 0.0),
        vec3(0.0, 0.0, 4.0),
    );
    assert_eq!(movement, vec3(0.0, 0.0, 4.0), "brush is not cover");
    let mut transform = Transform::from_xyz(0.0, 0.0, 3.0);
    assert!(turn_hull(&scene, &mut transform, 1.0), "nor stops a turn");
}

#[test]
fn round_crosses_a_tank_blocker_and_hits_the_target_behind_it() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_box(
        &mut scene,
        vec3(0.0, 0.0, 2.5),
        vec3(5.0, 1.0, 0.5),
        TANK_BLOCKER,
    );
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );
    let target = scene.create(None, None);
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 60.0,
            max_health: 60.0,
            ..Default::default()
        },
    );
    scene.prepare();

    // Through the blocker in one step, then on to the target.
    update_projectile(&mut scene, projectile, 0.35);
    scene.flush_deletes();
    assert!(
        scene.objects().any(|object| object == projectile),
        "the blocker lets the round through"
    );
    update_projectile(&mut scene, projectile, 0.5);
    scene.flush_deletes();

    let health = scene
        .read_component::<ComponentHealth, _, _>(target, |health| health.current_health)
        .unwrap();
    assert_eq!(health, 35.0, "the round hits the target behind the blocker");
}

#[test]
fn projectile_is_destroyed_when_it_reaches_a_wall() {
    let mut scene = calyx_engine::test_support::test_scene();
    add_fixed_cube(&mut scene, vec3(0.0, 0.0, 2.5), vec3(5.0, 1.0, 0.25));
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );
    scene.prepare();

    update_projectile(&mut scene, projectile, 0.1);
    scene.flush_deletes();
    assert!(
        scene.objects().any(|object| object == projectile),
        "the round has not reached the wall yet"
    );

    update_projectile(&mut scene, projectile, 0.5);
    scene.flush_deletes();
    assert!(!scene.objects().any(|object| object == projectile));
}

#[test]
fn projectile_expires_after_its_lifetime_in_open_space() {
    let mut scene = calyx_engine::test_support::test_scene();
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 25.0,
            lifetime_remaining: 0.5,
            radius: 0.1,
            ..Default::default()
        },
    );
    scene.prepare();

    update_projectile(&mut scene, projectile, 0.2);
    scene.flush_deletes();
    let remaining = scene
        .read_component::<ComponentProjectile, _, _>(projectile, |component| {
            component.lifetime_remaining
        })
        .expect("the round outlives its first update");
    assert!((remaining - 0.3).abs() < 1e-5, "remaining {remaining}");
    let position = scene.world_transform(projectile).position;
    assert!((position.z - 2.0).abs() < 1e-5, "position {position:?}");

    update_projectile(&mut scene, projectile, 0.2);
    scene.flush_deletes();
    assert!(scene.objects().any(|object| object == projectile));

    // The last update covers only the 0.1 s left, then removes the round.
    update_projectile(&mut scene, projectile, 0.2);
    scene.flush_deletes();
    assert!(!scene.objects().any(|object| object == projectile));
}

#[test]
fn lethal_projectile_hit_records_the_kill_for_its_shooter() {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = scene.create(
        Some(ComponentID {
            name: "Tank".to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.add_component(tank, ComponentTankController::default());
    scene.add_component(tank, ComponentCombatStats::default());
    scene.add_component(tank, ComponentKillFeed::default());
    let target = scene.create(
        Some(ComponentID {
            name: "Target A".to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 40.0,
            max_health: 40.0,
            destroy_on_death: true,
            ..Default::default()
        },
    );
    let fire = |scene: &mut calyx_engine::scene::Scene, damage: f32| {
        let projectile = scene.create(None, None);
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage,
                lifetime_remaining: 1.0,
                radius: 0.1,
                shooter: GameObjectRef::new(scene.uuid(tank)),
            },
        );
        update_projectile(scene, projectile, 0.5);
    };
    let kills = |scene: &calyx_engine::scene::Scene| {
        scene
            .read_component::<ComponentCombatStats, _, _>(tank, |stats| stats.kills)
            .unwrap()
    };

    fire(&mut scene, 25.0);
    assert_eq!(kills(&scene), 0, "a hit that does not kill is no kill");

    fire(&mut scene, 25.0);
    assert_eq!(kills(&scene), 1);
    let feed: Vec<(Option<String>, String)> = scene
        .read_component::<ComponentKillFeed, _, _>(tank, |feed| {
            feed.entries()
                .map(|entry| (entry.killer.clone(), entry.victim.clone()))
                .collect()
        })
        .unwrap();
    assert_eq!(
        feed,
        vec![(Some("Tank".to_string()), "Target A".to_string())]
    );
}

#[test]
fn lethal_projectile_hit_marks_health_dead_and_can_destroy_target() {
    let mut scene = calyx_engine::test_support::test_scene();
    let projectile = scene.create(None, None);
    scene.add_component(
        projectile,
        ComponentProjectile {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 10.0,
            damage: 100.0,
            lifetime_remaining: 1.0,
            radius: 0.1,
            ..Default::default()
        },
    );

    let target = scene.create(None, None);
    scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
    scene.add_component(target, ComponentProjectileTarget::default());
    scene.add_component(
        target,
        ComponentHealth {
            current_health: 40.0,
            max_health: 40.0,
            destroy_on_death: true,
            ..Default::default()
        },
    );

    update_projectile(&mut scene, projectile, 0.5);

    scene.flush_deletes();
    assert!(
        !scene.objects().any(|object| object == target),
        "destroy_on_death targets should be removed after lethal damage"
    );
}

#[test]
fn lethal_damage_respawns_player_and_restores_health() {
    let mut scene = calyx_engine::test_support::test_scene();
    let spawn = scene.create(None, None);
    let player = scene.create(None, None);
    scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
    scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
    scene.add_component(spawn, ComponentSpawnPoint::default());
    scene.add_component(
        player,
        ComponentHealth {
            max_health: 40.0,
            current_health: 40.0,
            ..Default::default()
        },
    );
    scene.add_component(
        player,
        ComponentRespawnState {
            respawn_delay: 1.0,
            ..Default::default()
        },
    );

    apply_projectile_damage(&mut scene, player, 40.0, None);

    assert!(scene
        .read_component::<ComponentHealth, _, _>(player, |health| health.dead)
        .expect("player should keep health while awaiting respawn"));
    assert!(scene
        .read_component::<ComponentRespawnState, _, _>(player, |state| state.death_requested)
        .expect("lethal damage should request a respawn"));

    assert!(!update_respawn_state(&mut scene, player, 0.0));
    assert!(update_respawn_state(&mut scene, player, 1.0));

    let health = scene
        .read_component::<ComponentHealth, _, _>(player, |health| *health)
        .expect("respawned player should keep health");
    assert_eq!(health.current_health, 40.0);
    assert!(!health.dead);
    assert_eq!(scene.world_transform(player).position, vec3(8.0, 0.0, 4.0));
}

#[test]
fn invulnerable_player_ignores_projectile_damage() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    scene.add_component(player, ComponentHealth::default());
    scene.add_component(
        player,
        ComponentRespawnState {
            invulnerability_remaining: 1.0,
            ..Default::default()
        },
    );

    apply_projectile_damage(&mut scene, player, 25.0, None);

    let health = scene
        .read_component::<ComponentHealth, _, _>(player, |health| *health)
        .expect("player should keep health");
    assert_eq!(health.current_health, health.max_health);
    assert!(!health.dead);
}

#[test]
fn waiting_to_respawn_player_ignores_projectile_damage() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    scene.add_component(player, ComponentHealth::default());
    scene.add_component(
        player,
        ComponentRespawnState {
            alive: false,
            respawn_remaining: 1.0,
            ..Default::default()
        },
    );

    apply_projectile_damage(&mut scene, player, 25.0, None);

    let health = scene
        .read_component::<ComponentHealth, _, _>(player, |health| *health)
        .expect("player should keep health");
    let state = scene
        .read_component::<ComponentRespawnState, _, _>(player, |state| *state)
        .expect("player should keep its respawn timer");
    assert_eq!(health.current_health, health.max_health);
    assert!(!state.death_requested);
    assert_eq!(state.respawn_remaining, 1.0);
}

#[test]
fn confirmed_damage_feedback_rejects_invalid_dead_and_immune_hits_and_clamps_overkill() {
    let mut scene = calyx_engine::test_support::test_scene();
    let shooter = scene.create(None, None);
    let victim = scene.create(None, None);
    scene.add_component(shooter, ComponentTankController::default());
    scene.add_component(shooter, ComponentCombatStats::default());
    scene.add_component(victim, ComponentHealth::default());
    for amount in [0.0, -2.0, f32::NAN, f32::INFINITY] {
        super::apply_projectile_damage(&mut scene, victim, amount, Some(shooter));
    }
    assert_eq!(
        scene.read_component::<ComponentHealth, _, _>(victim, |health| health.current_health),
        Some(100.0)
    );
    assert_eq!(
        scene.read_component::<ComponentCombatStats, _, _>(shooter, |stats| stats.feedback.len()),
        Some(0)
    );
    scene.add_component(
        victim,
        ComponentRespawnState {
            invulnerability_remaining: 1.0,
            ..Default::default()
        },
    );
    super::apply_projectile_damage(&mut scene, victim, 35.0, Some(shooter));
    assert_eq!(
        scene.read_component::<ComponentCombatStats, _, _>(shooter, |stats| stats.feedback.len()),
        Some(0)
    );
    scene.write_component::<ComponentRespawnState, _>(victim, |state| {
        state.invulnerability_remaining = 0.0
    });
    super::apply_projectile_damage(&mut scene, victim, 150.0, Some(shooter));
    super::apply_projectile_damage(&mut scene, victim, 150.0, Some(shooter));
    assert_eq!(
        scene.read_component::<ComponentCombatStats, _, _>(shooter, |stats| stats
            .feedback
            .iter()
            .map(|hit| hit.amount)
            .collect::<Vec<_>>()),
        Some(vec![100.0])
    );
    assert_eq!(
        scene.read_component::<ComponentCombatStats, _, _>(shooter, |stats| stats.kills),
        Some(1)
    );
}
