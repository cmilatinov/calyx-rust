use engine::context::GameContext;
use engine::math::Transform;
use engine::scene::{Scene, SceneHud};
use engine::test_support::{HeadlessSceneRunner, TestAssets};
use engine::ui::{PaintCommand, UiInput, UiPoint, UiRect, UiSize};
use nalgebra_glm::{vec3, Vec3};
use sandbox::spawn::ComponentRespawnState;
use sandbox::tank::{ComponentHealth, ComponentTankController};

/// Sandbox assets over a temporary copy of the asset roots, so loading them
/// never rewrites tracked `.meta` files. Keep the `TestAssets` alive while the
/// context is in use.
fn sandbox_assets() -> (TestAssets, engine::context::AssetContext) {
    let copy = TestAssets::game();
    let assets = copy.asset_context();
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    (copy, assets)
}

fn load_sandbox_scene(copy: &TestAssets, assets: &engine::context::AssetContext) -> Scene {
    assets
        .registries
        .assets
        .read()
        .reload_by_path::<Scene>(&copy.sandbox_root().join("scene.cxscene"))
        .expect("sandbox scene should load")
        .read()
        .clone()
}

fn object_named(scene: &Scene, name: &str) -> engine::scene::GameObject {
    scene
        .objects()
        .find(|game_object| scene.name(*game_object) == name)
        .unwrap_or_else(|| panic!("sandbox scene should contain {name}"))
}

fn assert_transform_eq(actual: Transform, expected: Transform) {
    assert_eq!(actual.position, expected.position);
    assert_eq!(actual.rotation, expected.rotation);
    assert_eq!(actual.scale, expected.scale);
}

#[test]
fn simulation_changes_do_not_leak_into_the_authoring_scene() {
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let mut game = GameContext::new(assets);
    game.scenes
        .load_scene(engine::core::Ref::new(scene).readonly());

    let tank = object_named(game.scenes.current_scene(), "Tank");
    let authoring_transform = game.scenes.current_scene().world_transform(tank);
    let authoring_count = game.scenes.current_scene().objects().count();

    game.scenes.start_simulation();
    let simulation_tank = object_named(game.scenes.simulation_scene(), "Tank");
    game.scenes.simulation_scene_mut().set_transform(
        simulation_tank,
        &nalgebra_glm::translation(&Vec3::new(9.0, 1.0, 4.0)),
    );
    game.scenes.stop_simulation();

    let tank = object_named(game.scenes.current_scene(), "Tank");
    assert_eq!(
        game.scenes.current_scene().objects().count(),
        authoring_count
    );
    assert_transform_eq(
        game.scenes.current_scene().world_transform(tank),
        authoring_transform,
    );
}

#[test]
fn tank_moves_without_changing_its_authored_scale() {
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let initial = scene.transform(tank);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::W);
    runner.step_many(30);
    runner.release_key(egui::Key::W);
    runner.step();

    let final_transform = runner.scene().transform(tank);
    assert_ne!(final_transform.position, initial.position);
    assert_eq!(final_transform.scale, initial.scale);
}

/// Moves `target` onto the tank barrel's firing line, `distance` metres ahead.
fn place_on_firing_line(scene: &mut Scene, target: engine::scene::GameObject, distance: f32) {
    let tank = object_named(scene, "Tank");
    let barrel = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            controller.barrel.game_object(scene)
        })
        .flatten()
        .expect("sandbox tank should reference its barrel");
    let barrel_transform = scene.world_transform(barrel);
    let mut forward = barrel_transform.forward();
    forward.y = 0.0;
    let mut target_transform = scene.world_transform(target);
    target_transform.position = barrel_transform.position + forward.normalize() * distance;
    scene.set_world_transform(target, target_transform.matrix());
}

fn current_health(scene: &Scene, object: engine::scene::GameObject) -> f32 {
    scene
        .read_component::<ComponentHealth, _, _>(object, |health| health.current_health)
        .expect("object should have health")
}

fn find_named(scene: &Scene, name: &str) -> Option<engine::scene::GameObject> {
    scene
        .objects()
        .find(|game_object| scene.name(*game_object) == name)
}

#[test]
fn player_rounds_damage_targets_until_they_are_destroyed() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let aimed_target = object_named(&scene, "Target A");
    // Close enough that the arena's centre cover, whose near face is 7 m
    // ahead of the spawn and absorbs rounds, is not in the way.
    place_on_firing_line(&mut scene, aimed_target, 4.0);
    let full_health = current_health(&scene, aimed_target);
    let other_target = object_named(&scene, "Target B");
    let other_transform = scene.world_transform(other_target);
    let other_health = current_health(&scene, other_target);
    let aimed_transform = scene.world_transform(aimed_target);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    let mut steps = 0;
    while current_health(runner.scene(), aimed_target) == full_health {
        runner.step();
        steps += 1;
        assert!(
            steps < 120,
            "a round should reach the target on the firing line"
        );
    }

    assert_eq!(
        current_health(runner.scene(), aimed_target),
        full_health - ComponentTankController::default().projectile_damage
    );
    assert_transform_eq(
        runner.scene().world_transform(aimed_target),
        aimed_transform,
    );
    assert_eq!(current_health(runner.scene(), other_target), other_health);
    assert_transform_eq(
        runner.scene().world_transform(other_target),
        other_transform,
    );

    runner.step_many(240);
    assert!(
        find_named(runner.scene(), "Target A").is_none(),
        "a target is removed once its health runs out"
    );
}

#[test]
fn sentry_kills_the_player_tank_which_respawns_after_the_delay() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let player = object_named(&scene, "Player");
    let spawn = scene.world_transform(object_named(&scene, "Player Spawn"));
    let sentry = scene.world_transform(object_named(&scene, "Sentry"));
    // Ten metres in front of the sentry, in its range and clear of cover.
    let mut rig = scene.world_transform(player);
    rig.position = vec3(sentry.position.x, 0.0, sentry.position.z - 10.0);
    scene.set_world_transform(player, rig.matrix());
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    let respawn_state = |runner: &HeadlessSceneRunner| {
        runner
            .scene()
            .read_component::<ComponentRespawnState, _, _>(tank, |state| *state)
            .expect("the tank should have respawn state")
    };
    let mut steps = 0;
    while respawn_state(&runner).alive {
        runner.step();
        steps += 1;
        assert!(steps < 60 * 15, "the sentry should kill the tank");
    }
    assert_eq!(current_health(runner.scene(), tank), 0.0);
    let death_position = runner.scene().world_transform(tank).position;

    // One second before the delay ends the tank still waits where it died.
    let delay = respawn_state(&runner).respawn_delay;
    let wait_steps = (delay * 60.0).round() as usize;
    runner.step_many(wait_steps - 60);
    assert!(!respawn_state(&runner).alive);
    assert_eq!(
        runner.scene().world_transform(tank).position,
        death_position
    );

    runner.step_many(61);
    let state = respawn_state(&runner);
    assert!(state.alive, "the tank should respawn after the delay");
    assert!((runner.scene().world_transform(tank).position - spawn.position).magnitude() < 1e-4);
    assert_eq!(current_health(runner.scene(), tank), 100.0);
}

#[test]
fn sandbox_scene_draws_the_tank_hud() {
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));

    let frame = SceneHud::default()
        .frame(&scene, viewport, UiInput::default())
        .expect("the sandbox scene should draw a HUD");

    let text: Vec<&str> = frame
        .paint_commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for expected in ["HULL INTEGRITY", "100/100", "6/6", "READY"] {
        assert!(text.contains(&expected), "{expected} in {text:?}");
    }
}

/// Yaw in radians of a world rotation, measured from +Z towards +X.
fn world_yaw(transform: &Transform) -> f32 {
    let forward = transform.forward();
    forward.x.atan2(forward.z)
}

fn angle_between(a: f32, b: f32) -> f32 {
    let difference = (a - b).rem_euclid(std::f32::consts::TAU);
    difference.min(std::f32::consts::TAU - difference)
}

/// Tank position, hull yaw and turret yaw at a moment of a drive.
#[derive(Debug, Clone, Copy)]
struct TankPose {
    time: f32,
    position: Vec3,
    hull_yaw: f32,
    turret_yaw: f32,
}

/// Drives the sandbox tank through the same input at `fps` frames per
/// second: forward and right with the cursor fixed off to one side for one
/// second, then no throttle. Returns the pose at each
/// checkpoint, in seconds.
fn drive_at_fps(fps: usize, checkpoints: &[f32]) -> Vec<TankPose> {
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let turret = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            controller.turret.game_object(&scene)
        })
        .flatten()
        .expect("sandbox tank should reference its turret");
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(1.0 / fps as f64);

    let release_step = fps;
    let pose = |runner: &HeadlessSceneRunner, step: usize| {
        let scene = runner.scene();
        let hull = scene.world_transform(tank);
        TankPose {
            time: step as f32 / fps as f32,
            position: hull.position,
            hull_yaw: world_yaw(&hull),
            turret_yaw: world_yaw(&scene.world_transform(turret)),
        }
    };

    runner.move_pointer(egui::pos2(1100.0, 150.0));
    runner.press_key(egui::Key::W);
    runner.press_key(egui::Key::D);
    let mut poses = Vec::new();
    let mut step = 0;
    for &checkpoint in checkpoints {
        let checkpoint_step = (checkpoint * fps as f32).round() as usize;
        while step < checkpoint_step {
            if step == release_step {
                runner.release_key(egui::Key::W);
                runner.release_key(egui::Key::D);
            }
            runner.step();
            step += 1;
        }
        poses.push(pose(&runner, step));
    }
    poses
}

#[test]
fn tank_handling_is_the_same_at_30_and_144_fps() {
    // Each checkpoint is a whole number of frames at both rates.
    let checkpoints = [1.0 / 6.0, 0.5, 1.0, 1.5, 2.5, 3.0];
    let slow = drive_at_fps(30, &checkpoints);
    let fast = drive_at_fps(144, &checkpoints);

    let mut mismatches = Vec::new();
    for (slow, fast) in slow.iter().zip(&fast) {
        let checks = [
            (
                "position",
                (slow.position - fast.position).magnitude(),
                0.05,
            ),
            (
                "hull yaw",
                angle_between(slow.hull_yaw, fast.hull_yaw),
                0.01,
            ),
            (
                "turret yaw",
                angle_between(slow.turret_yaw, fast.turret_yaw),
                0.01,
            ),
        ];
        for (name, difference, tolerance) in checks {
            if difference >= tolerance {
                mismatches.push(format!(
                    "{name} differs by {difference} at {}s\n  30 fps: {slow:?}\n  144 fps: {fast:?}",
                    slow.time
                ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));

    // The drive exercises the handling: the hull moved and turned, the turret
    // turned towards the cursor, and the hull coasted after the throttle.
    let [first, _, released, coasting, stopped, last] = fast[..] else {
        unreachable!()
    };
    assert!((released.position - first.position).magnitude() > 1.0);
    assert!(angle_between(released.hull_yaw, first.hull_yaw) > 1.0);
    assert!(angle_between(first.turret_yaw, first.hull_yaw) > 0.5);
    assert!((coasting.position - released.position).magnitude() > 0.5);
    assert!((last.position - stopped.position).magnitude() < 1e-4);
}
