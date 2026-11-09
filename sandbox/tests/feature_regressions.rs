// Matches the crates under test: the particle system component nests deep
// enough that proving it `Send`/`Sync` passes the default limit.
#![recursion_limit = "256"]

use calyx_engine::component::ComponentParticleSystem;
use calyx_engine::context::GameContext;
use calyx_engine::math::Transform;
use calyx_engine::scene::{Prefab, Scene, SceneHud};
use calyx_engine::test_support::{HeadlessSceneRunner, TestAssets};
use calyx_engine::ui::{PaintCommand, UiInput, UiPoint, UiRect, UiSize};
use calyx_engine::utils::TypeUuid;
use nalgebra_glm::{vec3, Vec3};
use sandbox::spawn::ComponentRespawnState;
use sandbox::tank::{ComponentHealth, ComponentTankController};

/// Sandbox assets over a temporary copy of the asset roots, so loading them
/// never rewrites tracked `.meta` files. Keep the `TestAssets` alive while the
/// context is in use.
fn sandbox_assets() -> (TestAssets, calyx_engine::context::AssetContext) {
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

/// The arena with its match already playing: it opens on the main menu,
/// where combat is paused (see `tests/match_flow.rs`).
fn load_sandbox_scene(copy: &TestAssets, assets: &calyx_engine::context::AssetContext) -> Scene {
    use sandbox::game_state::{ComponentGameState, GamePhase};
    let mut scene = assets
        .registries
        .assets
        .read()
        .reload_by_path::<Scene>(&copy.sandbox_root().join("game/scenes/arena.cxscene"))
        .expect("sandbox scene should load")
        .read()
        .clone();
    let state = object_named(&scene, "Game State");
    scene
        .write_component::<ComponentGameState, _>(state, |state| {
            state.transition_to(GamePhase::Playing);
        })
        .expect("the arena should carry a Game State");
    scene
}

fn object_named(scene: &Scene, name: &str) -> calyx_engine::scene::GameObject {
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
        .load_scene(calyx_engine::core::Ref::new(scene).readonly());

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
fn place_on_firing_line(scene: &mut Scene, target: calyx_engine::scene::GameObject, distance: f32) {
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

fn current_health(scene: &Scene, object: calyx_engine::scene::GameObject) -> f32 {
    scene
        .read_component::<ComponentHealth, _, _>(object, |health| health.current_health)
        .expect("object should have health")
}

fn find_named(scene: &Scene, name: &str) -> Option<calyx_engine::scene::GameObject> {
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
            PaintCommand::Label(label) => Some(label.text.as_str()),
            _ => None,
        })
        .collect();
    for expected in ["N", "05:00"] {
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

/// Drives the sandbox tank with throttle only for `seconds` at `fps`, from
/// `start` facing `yaw` radians, and returns where it ends up.
fn drive_from(start: Vec3, yaw: f32, fps: usize, seconds: f32) -> Vec3 {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let mut transform = scene.world_transform(tank);
    transform.position = start;
    transform.rotation = nalgebra::UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
    scene.set_world_transform(tank, transform.matrix());
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(1.0 / fps as f64);
    runner.press_key(egui::Key::W);
    runner.step_many((seconds * fps as f32).round() as usize);
    runner.scene().world_transform(tank).position
}

#[test]
fn tank_slides_along_a_wall_the_same_at_30_and_144_fps() {
    // Facing 45 degrees into the arena's +X perimeter wall, whose face is at
    // x = 28.4, in the lane between it and the cover.
    let start = vec3(26.0, 0.0, 0.0);
    let yaw = std::f32::consts::FRAC_PI_4;
    let slow = drive_from(start, yaw, 30, 1.5);
    let fast = drive_from(start, yaw, 144, 1.5);

    assert!(
        (slow - fast).magnitude() < 0.05,
        "30 fps: {slow:?}\n144 fps: {fast:?}"
    );
    assert!(fast.z - start.z > 2.0, "the tank should slide: {fast:?}");
}

#[test]
fn arena_ground_covers_camera_corners_at_zoom_and_aim_limits() {
    use calyx_engine::component::{ComponentCamera, ComponentCollider, ComponentTerrain};
    use nalgebra_glm::vec2;
    use sandbox::{arena::ComponentArena, tank_camera::ComponentTankCamera};

    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let ground = object_named(&scene, "Arena Ground");
    let world = scene.world_transform(ground);
    let (size, height) = scene
        .read_component::<ComponentTerrain, _, _>(ground, |t| (t.size, t.height_scale))
        .unwrap();
    // The terrain is upright and unscaled: its rectangle, and its top at the
    // ground's level.
    let min = world.position - vec3(size.x, 0.0, size.y) * 0.5;
    let max = world.position + vec3(size.x * 0.5, height, size.y * 0.5);
    assert!(max.y.abs() < 1e-4, "the ground's top is at y = 0: {max:?}");
    assert!(scene
        .read_component::<ComponentCollider, _, _>(ground, |_| ())
        .is_none());
    let arena = object_named(&scene, "Arena");
    let center = scene.world_transform(arena).position;
    let half = scene
        .read_component::<ComponentArena, _, _>(arena, |a| {
            vec3(
                a.columns as f32 * a.cell_size,
                0.0,
                a.rows as f32 * a.cell_size,
            ) * 0.5
        })
        .unwrap();
    let camera = object_named(&scene, "Camera");
    let rig = scene
        .read_component::<ComponentTankCamera, _, _>(camera, Clone::clone)
        .unwrap();
    for aspect in [4.0 / 3.0, 16.0 / 9.0, 32.0 / 9.0] {
        let viewport = vec2(900.0 * aspect, 900.0);
        let lens = scene
            .read_component::<ComponentCamera, _, _>(camera, |c| c.camera(aspect))
            .unwrap();
        for zoom in [rig.min_zoom, rig.max_zoom] {
            for x in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    // Include the conservative square around the entire
                    // aim-lead disk, even at the outer edges of the walls.
                    let focus = center
                        + vec3(
                            x * (half.x + rig.aim_ahead_max),
                            0.0,
                            z * (half.z + rig.aim_ahead_max),
                        );
                    let transform = rig.camera_transform(focus, 0.0, zoom);
                    for corner in [
                        vec2(0.0, 0.0),
                        vec2(viewport.x, 0.0),
                        vec2(0.0, viewport.y),
                        viewport,
                    ] {
                        let ray = lens.screen_ray(&transform, corner, viewport).unwrap();
                        let distance = (max.y - ray.origin.y) / ray.direction.y;
                        assert!(distance > 0.0 && distance < lens.far_plane);
                        let hit = ray.origin + ray.direction * distance;
                        assert!(
                            hit.x >= min.x && hit.x <= max.x && hit.z >= min.z && hit.z <= max.z,
                            "camera corner exposes sky at {hit:?}, aspect {aspect}, zoom {zoom}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_arena_ground_is_one_surface_with_nothing_coplanar() {
    use calyx_engine::component::ComponentTerrain;

    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let terrains: Vec<_> = scene
        .objects()
        .filter(|&o| {
            scene
                .read_component::<ComponentTerrain, _, _>(o, |_| ())
                .is_some()
        })
        .collect();
    assert_eq!(terrains.len(), 1, "one ground terrain");
    assert_eq!(scene.name(terrains[0]), "Arena Ground");
    let arena_y = scene
        .world_transform(object_named(&scene, "Arena"))
        .position
        .y;
    // The only ground tiles left are the spawn's paving and the road, raised
    // off the ground: 0.1 m tiles whose tops sit 3 cm above it.
    let tiles: Vec<_> = scene
        .objects()
        .filter(|&o| {
            let name = scene.name(o);
            name.starts_with("Arena Ground ")
        })
        .collect();
    assert!(
        !tiles.is_empty() && tiles.len() < 80,
        "{} ground tiles",
        tiles.len()
    );
    for tile in tiles {
        let top = scene.world_transform(tile).position.y - arena_y + 0.1;
        assert!(
            (top - 0.03).abs() < 1e-4,
            "{} lies at {top}, not 3 cm over the ground",
            scene.name(tile)
        );
    }
    for overlay in ["Crop Row", "Creek Verge", "Arena Foundation"] {
        assert!(
            !scene.objects().any(|o| scene.name(o).contains(overlay)),
            "no {overlay} objects lie on the ground"
        );
    }
}

#[test]
fn authored_arena_colliders_close_wall_joins_and_stop_shots_at_cover() {
    use rapier3d::pipeline::QueryFilter;
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    // What stops rounds: the farmland creek's blockers, which stop only
    // tanks, are not among it.
    let rounds = QueryFilter::only_fixed().groups(sandbox::collision::ROUNDS.into());
    let hit_distance = |origin, direction, length| {
        runner
            .scene()
            .physics
            .cast_ray(origin, direction, length, true, rounds)
            .unwrap_or_else(|| panic!("missing collider from {origin:?} toward {direction:?}"))
            .toi
    };
    // Probe every one-metre wall join from immediately inside the pen.
    for x in -26..=26 {
        for (z, direction) in [(-8.0, -1.0), (52.0, 1.0)] {
            let distance = hit_distance(vec3(x as f32, 0.8, z), vec3(0.0, 0.0, direction), 1.0);
            assert!(
                (distance - 0.4).abs() < 0.005,
                "wall at x={x}, z={z}: {distance}"
            );
        }
    }
    for z in -5..=49 {
        for side in [-1.0, 1.0] {
            let distance =
                hit_distance(vec3(side * 28.3, 0.8, z as f32), vec3(side, 0.0, 0.0), 1.0);
            assert!((distance - 0.1).abs() < 0.005, "wall at z={z}: {distance}");
        }
    }
    // Every solid rises to the shell-blocking top, so the barrier's widest
    // foot band now meets a shot at gun height first.
    let distance = hit_distance(vec3(0.0, 0.8, 16.0), vec3(0.0, 0.0, 1.0), 6.0);
    assert!((distance - 3.44).abs() < 0.01, "centre barrier: {distance}");
    // The rotated sandbag stack is a thin pile, not a full-depth
    // rectangular block. A shot meets the bottom row, risen to the
    // shell-blocking top, but air beside the pile must stay clear.
    let sandbags = object_named(runner.scene(), "Arena Cover Right Sandbags");
    let sandbag_pose = runner.scene().world_transform(sandbags);
    let point = |p: Vec3| (sandbag_pose.matrix() * nalgebra_glm::vec4(p.x, p.y, p.z, 1.0)).xyz();
    let direction = |d| sandbag_pose.rotation * d;
    let distance = hit_distance(
        point(vec3(0.0, 0.65, -1.0)),
        direction(vec3(0.0, 0.0, 1.0)),
        2.0,
    );
    assert!((distance - 0.785).abs() < 0.01, "sandbag pile: {distance}");
    let beside = point(vec3(2.0, 0.6, 0.35));
    assert!(
        runner
            .scene()
            .physics
            .cast_ray(
                beside,
                direction(vec3(-1.0, 0.0, 0.0)),
                4.0,
                true,
                QueryFilter::only_fixed()
            )
            .is_none(),
        "invisible sandbag collision from {beside:?}"
    );
    // Flat tile seams and paving gaps share the same continuous floor.
    for x in [-9.0, -0.5, 0.0, 0.5, 9.0] {
        let distance = hit_distance(vec3(x, 0.1, 1.0), vec3(0.0, -1.0, 0.0), 1.0);
        assert!(
            (distance - 0.1).abs() < 0.001,
            "ground at x={x}: {distance}"
        );
    }
}

#[test]
fn authored_rock_shoulders_stop_shells_and_crowns_leave_air() {
    use calyx_engine::component::ColliderShape;
    use rapier3d::pipeline::QueryFilter;
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let boulder = object_named(&scene, "Arena Cover West Boulder");
    let transform = scene.world_transform(boulder);
    let point = |p: Vec3| (transform.matrix() * nalgebra_glm::vec4(p.x, p.y, p.z, 1.0)).xyz();
    let direction = transform.rotation * vec3(0.0, 0.0, 1.0);
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    // The exported body crosses this local line at Z=-.0415 and .2691.
    // A normal shell must hit that shoulder, even though it misses a
    // narrow, centred approximation of the rock's middle tier.
    assert!(
        runner
            .scene()
            .physics
            .cast_shape(
                ColliderShape::Sphere { radius: 0.18 },
                point(vec3(0.9, 1.1, -0.5)),
                nalgebra::UnitQuaternion::identity(),
                direction,
                1.0,
                QueryFilter::only_fixed(),
            )
            .is_some(),
        "shell passed through the visible rock shoulder"
    );
    // Beside the crown, at gun height, the rock still stops the round
    // rather than letting it fly over the lower shoulder.
    assert!(
        runner
            .scene()
            .physics
            .cast_shape(
                ColliderShape::Sphere { radius: 0.18 },
                point(vec3(0.35, 1.61, -0.5)),
                nalgebra::UnitQuaternion::identity(),
                direction,
                1.2,
                QueryFilter::only_fixed(),
            )
            .is_some(),
        "shell flew over the rock beside its crown"
    );
}

/// No obstacle is low enough for a round fired from the tank's gun to fly
/// over it: every cover, prop and wall solid rises above the shell's path,
/// measured from the ground the obstacle stands on, so cover on the raised
/// deck stops rounds fired across the deck. Ramps and kerbs are driving
/// surfaces, not cover, and are not among these solids.
#[test]
fn shells_at_gun_height_cannot_fly_over_any_obstacle() {
    use calyx_engine::component::ColliderShape;
    use rapier3d::pipeline::QueryFilter;
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let barrel = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            controller.barrel.game_object(&scene)
        })
        .flatten()
        .expect("sandbox tank should reference its barrel");
    let gun_height = scene.world_transform(barrel).position.y;
    let solids: Vec<_> = scene
        .objects()
        .filter(|object| {
            let name = scene.name(*object);
            name.starts_with("Arena ") && name.contains(" collision")
        })
        .collect();
    assert!(solids.len() > 80, "{} obstacle solids", solids.len());
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    for solid in solids {
        let center = runner.scene().world_transform(solid).position;
        let ground = runner
            .scene()
            .parent(solid)
            .map_or(0.0, |prop| runner.scene().world_transform(prop).position.y);
        let origin = vec3(center.x - 3.0, ground + gun_height, center.z);
        assert!(
            runner
                .scene()
                .physics
                .cast_shape(
                    ColliderShape::Sphere { radius: 0.18 },
                    origin,
                    nalgebra::UnitQuaternion::identity(),
                    vec3(1.0, 0.0, 0.0),
                    3.0,
                    QueryFilter::only_fixed(),
                )
                .is_some(),
            "a shell at gun height flew over {}",
            runner.scene().name(solid)
        );
    }
}

/// A sentry standing on the ground takes the player's rounds, which fly
/// well above its origin, and is destroyed once its health runs out.
#[test]
fn player_rounds_destroy_a_sentry_on_the_ground() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let sentry = object_named(&scene, "Sentry");
    let ground = scene.world_transform(sentry).position.y;
    place_on_firing_line(&mut scene, sentry, 4.0);
    let mut pose = scene.world_transform(sentry);
    pose.position.y = ground;
    scene.set_world_transform(sentry, pose.matrix());
    let full_health = current_health(&scene, sentry);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    let mut steps = 0;
    while current_health(runner.scene(), sentry) == full_health {
        runner.step();
        steps += 1;
        assert!(steps < 120, "a round should hit the sentry");
    }
    assert_eq!(
        current_health(runner.scene(), sentry),
        full_health - ComponentTankController::default().projectile_damage
    );
    while find_named(runner.scene(), "Sentry").is_some() {
        runner.step();
        steps += 1;
        assert!(steps < 600, "the sentry should be destroyed");
    }
}

#[test]
fn both_authored_flank_lanes_are_driveable() {
    for x in [-20.0, 20.0] {
        let final_position = drive_from(vec3(x, 0.0, 0.0), 0.0, 60, 6.0);
        assert!(
            final_position.z > 37.0,
            "flank x={x} blocked at {final_position:?}"
        );
        assert!(
            (final_position.x - x).abs() < 0.1,
            "unexpected deflection: {final_position:?}"
        );
    }
}

/// Fails when `authored` names a key `known` lacks, at any depth, so a
/// misspelt particle module field cannot silently fall back to its default.
fn assert_known_keys(authored: &serde_json::Value, known: &serde_json::Value, path: &str) {
    if let (Some(authored), Some(known)) = (authored.as_object(), known.as_object()) {
        for (key, value) in authored {
            let field = format!("{path}.{key}");
            let known_value = known
                .get(key)
                .unwrap_or_else(|| panic!("unknown particle field {field}"));
            assert_known_keys(value, known_value, &field);
        }
    }
}

#[test]
fn campfire_prefab_loads_with_every_particle_module_field_known() {
    let (copy, assets) = sandbox_assets();
    let path = copy.sandbox_root().join("shared/prefabs/campfire.cxprefab");
    let source: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let particle_type = ComponentParticleSystem::type_uuid().to_string();
    let mut authored_systems = 0;
    for components in source["data"]["components"].as_object().unwrap().values() {
        if let Some(authored) = components.get(&particle_type) {
            let system: ComponentParticleSystem = serde_json::from_value(authored.clone())
                .expect("campfire particle system should deserialize");
            let known = serde_json::to_value(&system).unwrap();
            assert_known_keys(authored, &known, "particle_system");
            authored_systems += 1;
        }
    }
    assert_eq!(authored_systems, 3);

    let prefab = assets
        .registries
        .assets
        .read()
        .reload_by_path::<Prefab>(&path)
        .expect("campfire prefab should load");
    let mut scene = load_sandbox_scene(&copy, &assets);
    let before = scene.objects().count();
    let (_, dropped) = scene
        .instantiate_prefab_reporting(&prefab.read(), None)
        .expect("campfire prefab should instantiate");
    assert!(
        dropped.is_empty(),
        "campfire dropped components: {dropped:?}"
    );
    assert_eq!(scene.objects().count(), before + 4);
}

#[test]
fn sandbox_campfire_emits_from_each_of_its_systems() {
    let (copy, assets) = sandbox_assets();
    // The arena no longer places a campfire; drop the shared prefab into it.
    let prefab = assets
        .registries
        .assets
        .read()
        .reload_by_path::<Prefab>(&copy.sandbox_root().join("shared/prefabs/campfire.cxprefab"))
        .expect("campfire prefab should load");
    let mut scene = load_sandbox_scene(&copy, &assets);
    scene
        .instantiate_prefab_reporting(&prefab.read(), None)
        .expect("campfire prefab should instantiate");
    let puff = assets
        .registries
        .assets
        .read()
        .asset_id("shared/textures/particle_puff")
        .expect("the puff texture should be a sandbox asset");
    let systems = ["Flames", "Flame Core", "Smoke"].map(|name| object_named(&scene, name));
    let mut runner = HeadlessSceneRunner::from_scene(scene).with_particles();
    runner.step_many(30);

    for system in systems {
        let texture = runner
            .scene()
            .read_component::<ComponentParticleSystem, _, _>(system, |system| {
                serde_json::to_value(&system.rendering.texture).unwrap()
            })
            .expect("campfire child should hold a particle system");
        let count = runner
            .particles()
            .and_then(|particles| particles.live_count(system))
            .expect("the simulation should run each campfire system");
        assert!(
            count > 0,
            "{} should have live particles",
            runner.scene().name(system)
        );
        assert_eq!(texture, serde_json::json!(puff));
    }
}

/// Collects the string literals passed to `.{method}("...")` in every source file
/// under `dir`.
fn names_read_by(dir: &std::path::Path, method: &str, names: &mut Vec<String>) {
    let call = format!(".{method}(\"");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            names_read_by(&path, method, names);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            let mut remaining = source.as_str();
            while let Some(start) = remaining.find(&call) {
                remaining = &remaining[start + call.len()..];
                let end = remaining.find('"').unwrap();
                names.push(remaining[..end].to_string());
            }
        }
    }
}

#[test]
fn sandbox_action_map_binds_every_action_and_axis_the_game_reads() {
    let (_copy, assets) = sandbox_assets();
    let map = assets
        .registries
        .assets
        .read()
        .load::<calyx_engine::input::ActionMap>(calyx_engine::input::PROJECT_ACTION_MAP_NAME)
        .expect("the sandbox should ship input.cxinput");
    let map = map.read();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let input = egui::InputState::default();

    let mut actions = Vec::new();
    names_read_by(&source, "action", &mut actions);
    assert!(actions.contains(&"shoot".to_string()));
    assert!(actions.contains(&"scoreboard".to_string()));
    for name in actions {
        assert!(map.action(&name, &input).is_some(), "unbound action {name}");
    }

    let mut axes = Vec::new();
    names_read_by(&source, "axis", &mut axes);
    assert!(axes.contains(&"move_forward".to_string()));
    for name in axes {
        assert!(map.axis(&name, &input).is_some(), "unbound axis {name}");
    }
}

/// A sandbox mesh's vertex positions and triangles (faces fanned), read from its OBJ.
fn obj_mesh(name: &str) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/game/meshes")
        .join(format!("{name}.obj"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} does not read: {e}", path.display()));
    let mut positions = Vec::new();
    let mut triangles = Vec::new();
    for line in source.lines() {
        if let Some(position) = line.strip_prefix("v ") {
            let mut coordinates = position
                .split_whitespace()
                .map(|value| value.parse::<f32>().unwrap());
            positions.push(vec3(
                coordinates.next().unwrap(),
                coordinates.next().unwrap(),
                coordinates.next().unwrap(),
            ));
        } else if let Some(face) = line.strip_prefix("f ") {
            let corners: Vec<usize> = face
                .split_whitespace()
                .map(|corner| corner.split('/').next().unwrap().parse::<usize>().unwrap() - 1)
                .collect();
            for i in 1..corners.len() - 1 {
                triangles.push([corners[0], corners[i], corners[i + 1]]);
            }
        }
    }
    (positions, triangles)
}

fn bounds(points: impl IntoIterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.into_iter().fold(
        (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
        |(min, max), point| (min.inf(&point), max.sup(&point)),
    )
}

/// Whether segment `p`..`q` passes through the interior of triangle `t`. A
/// segment that only touches the triangle's plane or edge, within a small
/// tolerance, does not count, so faces that merely meet are not intersections.
fn segment_pierces_triangle(p: Vec3, q: Vec3, t: [Vec3; 3]) -> bool {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let normal = e1.cross(&e2);
    if normal.norm() < 1e-12 {
        return false;
    }
    let normal = normal.normalize();
    let (dp, dq) = ((p - t[0]).dot(&normal), (q - t[0]).dot(&normal));
    if !((dp > 1e-5 && dq < -1e-5) || (dp < -1e-5 && dq > 1e-5)) {
        return false;
    }
    let x = p + (q - p) * (dp / (dp - dq)) - t[0];
    let (d00, d01, d11) = (e1.dot(&e1), e1.dot(&e2), e2.dot(&e2));
    let (d20, d21) = (x.dot(&e1), x.dot(&e2));
    let denominator = d00 * d11 - d01 * d01;
    let v = (d11 * d20 - d01 * d21) / denominator;
    let w = (d00 * d21 - d01 * d20) / denominator;
    (1.0 - v - w).min(v).min(w) > 1e-4
}

/// Whether two triangles interpenetrate: an edge of one passes through the other.
fn triangles_intersect(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    (0..3).any(|i| segment_pierces_triangle(a[i], a[(i + 1) % 3], b))
        || (0..3).any(|i| segment_pierces_triangle(b[i], b[(i + 1) % 3], a))
}

/// How many triangle pairs of two sandbox meshes interpenetrate.
fn intersecting_triangle_pairs(a: &str, b: &str) -> usize {
    let (positions_a, triangles_a) = obj_mesh(a);
    let (positions_b, triangles_b) = obj_mesh(b);
    let corners = |positions: &[Vec3], t: &[usize; 3]| t.map(|i| positions[i]);
    let (min_b, max_b) = bounds(positions_b.iter().copied());
    let boxed_b: Vec<_> = triangles_b
        .iter()
        .map(|t| {
            let c = corners(&positions_b, t);
            (c, bounds(c))
        })
        .collect();
    let mut count = 0;
    for t in &triangles_a {
        let c = corners(&positions_a, t);
        let (min, max) = bounds(c);
        if (max.inf(&max_b) - min.sup(&min_b)).min() < 0.0 {
            continue;
        }
        for (other, (other_min, other_max)) in &boxed_b {
            let overlap = max.inf(other_max) - min.sup(other_min);
            if overlap.min() >= 0.0 && triangles_intersect(c, *other) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn tank_right_side_meshes_mirror_the_left_ones_across_the_centre_plane() {
    // The tank prefab places every part with an identity transform, so each
    // right-side mesh must be its left twin reflected across x = 0.
    for part in ["wheels", "tracks"] {
        let (left_min, left_max) = bounds(obj_mesh(&format!("{part}_left")).0);
        let (right_min, right_max) = bounds(obj_mesh(&format!("{part}_right")).0);
        let mirrored_min = vec3(-left_max.x, left_min.y, left_min.z);
        let mirrored_max = vec3(-left_min.x, left_max.y, left_max.z);
        assert!(
            (right_min - mirrored_min).abs().max() < 1e-3
                && (right_max - mirrored_max).abs().max() < 1e-3,
            "{part}_right bounds {right_min:?}..{right_max:?} should mirror \
             {part}_left's: {mirrored_min:?}..{mirrored_max:?}"
        );
    }
}

#[test]
fn tank_hull_clears_the_wheels_and_tracks() {
    // As in the authored Tank_002.fbx, the hull sits above the running gear:
    // no hull triangle cuts into a wheel or a track.
    for part in ["wheels_left", "wheels_right", "tracks_left", "tracks_right"] {
        assert_eq!(
            intersecting_triangle_pairs("body", part),
            0,
            "the hull intersects {part}"
        );
    }
}

fn descendant_named(
    scene: &Scene,
    root: calyx_engine::scene::GameObject,
    name: &str,
) -> calyx_engine::scene::GameObject {
    scene
        .descendants(root)
        .find(|game_object| scene.name(*game_object) == name)
        .unwrap_or_else(|| panic!("{} should contain {name}", scene.name(root)))
}

/// The bounds of a tank part's mesh in the tank's own space, where the
/// source model's layout is measured.
fn tank_space_bounds(
    scene: &Scene,
    tank: calyx_engine::scene::GameObject,
    part: &str,
    mesh: &str,
) -> (Vec3, Vec3) {
    let part = descendant_named(scene, tank, part);
    let to_tank =
        scene.world_transform(tank).inverse_matrix() * scene.world_transform(part).matrix();
    bounds(
        obj_mesh(mesh)
            .0
            .into_iter()
            .map(|p| (to_tank * nalgebra_glm::vec4(p.x, p.y, p.z, 1.0)).xyz()),
    )
}

/// Asserts a tank's parts sit where Tank_002.fbx puts them: its bounds, read
/// in Blender and moved into the game's Y-up tank space (with the export's
/// 0.289567 lift that the tracks and wheels carry).
fn assert_tank_matches_the_source_layout(scene: &Scene, tank: calyx_engine::scene::GameObject) {
    let near = |actual: f32, expected: f32, what: &str| {
        assert!(
            (actual - expected).abs() < 0.01,
            "{what} is {actual}, the source model has {expected}"
        );
    };
    let full = [
        (
            "Body",
            "body",
            [-2.5639, 0.4759, -3.4473],
            [2.5639, 2.6559, 2.5436],
        ),
        (
            "Turret Pivot",
            "turret_pivot",
            [-0.8379, 2.4479, -0.5675],
            [0.8666, 3.0172, 1.1374],
        ),
        (
            "Gun",
            "gun",
            [-0.3991, 3.2214, 1.2124],
            [0.3738, 3.9846, 4.5622],
        ),
    ];
    for (part, mesh, min, max) in full {
        let (actual_min, actual_max) = tank_space_bounds(scene, tank, part, mesh);
        for axis in 0..3 {
            near(actual_min[axis], min[axis], &format!("{part} min[{axis}]"));
            near(actual_max[axis], max[axis], &format!("{part} max[{axis}]"));
        }
    }
    // The exported head lacks some low rear geometry of the source head, so
    // only the corners that geometry does not reach are compared.
    let (min, max) = tank_space_bounds(scene, tank, "Turret Head", "turret_head");
    near(min.x, -1.437, "Turret Head min x");
    near(max.x, 1.437, "Turret Head max x");
    near(max.y, 6.2127, "Turret Head top");
    near(max.z, 1.9523, "Turret Head front");
}

#[test]
fn tank_turret_and_gun_sit_where_the_source_model_puts_them() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let arena_tank = object_named(&scene, "Tank");
    assert_tank_matches_the_source_layout(&scene, arena_tank);

    let prefab = assets
        .registries
        .assets
        .read()
        .reload_by_path::<Prefab>(&copy.sandbox_root().join("game/prefabs/tank.cxprefab"))
        .expect("tank prefab should load");
    let (root, _) = scene
        .instantiate_prefab_reporting(&prefab.read(), None)
        .expect("tank prefab should instantiate");
    let prefab_tank = descendant_named(&scene, root, "Tank");
    assert_tank_matches_the_source_layout(&scene, prefab_tank);
}

#[test]
fn tank_rounds_leave_from_the_barrel_tip() {
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let barrel = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            controller.barrel.game_object(&scene)
        })
        .flatten()
        .expect("sandbox tank should reference its barrel");
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    let mut steps = 0;
    let (round, projectile) = loop {
        runner.step();
        let scene = runner.scene();
        if let Some(round) = find_named(scene, "Projectile") {
            let projectile = scene
                .read_component::<sandbox::tank::ComponentProjectile, _, _>(round, |p| *p)
                .unwrap();
            break (round, projectile);
        }
        steps += 1;
        assert!(steps < 60, "the tank should fire a round");
    };
    let scene = runner.scene();
    let controller = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| controller.clone())
        .unwrap();
    // Where the round started, before it flew for the time it has used.
    let flown = controller.projectile_lifetime - projectile.lifetime_remaining;
    let start =
        scene.world_transform(round).position - projectile.direction * projectile.speed * flown;

    let barrel_transform = scene.world_transform(barrel);
    let to_world = barrel_transform.matrix();
    let gun: Vec<Vec3> = obj_mesh("gun")
        .0
        .into_iter()
        .map(|p| (to_world * nalgebra_glm::vec4(p.x, p.y, p.z, 1.0)).xyz())
        .collect();
    let forward = projectile.direction;
    let along = |p: &Vec3| (p - barrel_transform.position).dot(&forward);
    let tip = gun.iter().map(along).fold(f32::NEG_INFINITY, f32::max);
    let muzzle: Vec<&Vec3> = gun.iter().filter(|p| along(p) > tip - 0.01).collect();
    let muzzle_centre = muzzle.iter().copied().sum::<Vec3>() / muzzle.len() as f32;

    let past_tip = along(&start) - tip;
    assert!(
        (0.0..=projectile.radius).contains(&past_tip),
        "a round should start at the barrel tip, not {past_tip} m past it"
    );
    let off_bore = (start - muzzle_centre) - forward * (start - muzzle_centre).dot(&forward);
    assert!(
        off_bore.norm() < 0.05,
        "a round should start on the barrel's bore, not {} m off it",
        off_bore.norm()
    );
}

fn hud_text(scene: &Scene) -> Vec<String> {
    let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));
    let frame = SceneHud::default()
        .frame(scene, viewport, UiInput::default())
        .expect("the sandbox scene should draw a HUD");
    frame
        .paint_commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.clone()),
            PaintCommand::Label(label) => Some(label.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_arena_match_ends_at_its_kill_limit_with_the_final_scoreboard() {
    use sandbox::combat::ComponentCombatStats;
    use sandbox::match_rules::{match_over, ComponentMatchRules, MatchMode};

    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let rules = object_named(&scene, "Match Rules");
    let authored = scene
        .read_component::<ComponentMatchRules, _, _>(rules, Clone::clone)
        .expect("the arena should carry its match rules");
    assert_eq!(authored.mode(), MatchMode::TeamDeathmatch);
    assert_eq!((authored.kill_limit, authored.time_limit), (20, 300.0));
    scene.write_component::<ComponentMatchRules, _>(rules, |rules| rules.kill_limit = 1);
    let target = object_named(&scene, "Target A");
    place_on_firing_line(&mut scene, target, 4.0);
    let tank = object_named(&scene, "Tank");
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    let mut steps = 0;
    while !match_over(runner.scene()) {
        runner.step();
        steps += 1;
        assert!(
            steps < 600,
            "destroying Target A should reach the kill limit"
        );
    }
    let kills = |runner: &HeadlessSceneRunner| {
        runner
            .scene()
            .read_component::<ComponentCombatStats, _, _>(tank, |stats| stats.kills)
            .unwrap()
    };
    assert_eq!(kills(&runner), 1);
    let text = hud_text(runner.scene());
    for expected in ["SCOREBOARD", "VICTORY", "KILL LIMIT REACHED", "Tank"] {
        assert!(text.iter().any(|t| t == expected), "{expected} in {text:?}");
    }

    // The match stays over: the tank holds still and no more rounds fly.
    let position = runner.scene().world_transform(tank).position;
    runner.press_key(egui::Key::W);
    runner.step_many(60);
    assert_eq!(runner.scene().world_transform(tank).position, position);
    assert_eq!(kills(&runner), 1);
}

// The raised area: a platform as high as the walls in the arena's northeast,
// at world (20, 0, 45), its deck 1.6 m up, reached by a ramp from the south
// (continuing the east sand lane at x = 20) and a ramp from the west (at
// z = 46). Ramps climb 1.6 m over 8 m between kerbs.
const DECK_HEIGHT: f32 = 1.6;

/// The arena with its tank at `start` facing `yaw` radians, and without the
/// sentry, whose rounds would otherwise interrupt a drive near it.
fn raised_area_runner(
    start: Vec3,
    yaw: f32,
    fps: usize,
) -> (HeadlessSceneRunner, calyx_engine::scene::GameObject) {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let sentry = object_named(&scene, "Sentry");
    scene.delete(sentry);
    scene.flush_deletes();
    let tank = object_named(&scene, "Tank");
    let mut transform = scene.world_transform(tank);
    transform.position = start;
    transform.rotation = nalgebra::UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
    scene.set_world_transform(tank, transform.matrix());
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(1.0 / fps as f64);
    (runner, tank)
}

/// Holds `key` (or nothing) for `seconds` at `fps`.
fn hold(runner: &mut HeadlessSceneRunner, key: Option<egui::Key>, seconds: f32, fps: usize) {
    if let Some(key) = key {
        runner.press_key(key);
    }
    runner.step_many((seconds * fps as f32).round() as usize);
    if let Some(key) = key {
        runner.release_key(key);
        runner.step();
    }
}

/// The hull's nose-up pitch, in radians.
fn hull_pitch(transform: &Transform) -> f32 {
    transform.forward().normalize().y.asin()
}

#[test]
fn tank_climbs_the_south_ramp_onto_the_platform_and_drives_back_down() {
    // At the south end of the east sand lane, facing up the ramp.
    let (mut runner, tank) = raised_area_runner(vec3(20.0, 0.0, 26.0), 0.0, 60);
    let mut climbing = Vec::new();
    runner.press_key(egui::Key::W);
    for _ in 0..240 {
        runner.step();
        climbing.push(runner.scene().world_transform(tank));
    }
    runner.release_key(egui::Key::W);
    hold(&mut runner, None, 2.0, 60);

    // Halfway up the ramp the hull is halfway up and tilted to the slope.
    let halfway = climbing
        .iter()
        .find(|pose| pose.position.z >= 37.0)
        .expect("the tank should reach the middle of the ramp");
    assert!(
        (halfway.position.y - DECK_HEIGHT * (halfway.position.z - 33.0) / 8.0).abs() < 0.05,
        "halfway up the ramp at {:?}",
        halfway.position
    );
    let slope = (DECK_HEIGHT / 8.0).atan();
    assert!(
        (hull_pitch(halfway) - slope).abs() < 0.02,
        "pitch {}",
        hull_pitch(halfway)
    );
    // The height never jumps: the tank rides the ramp without snagging.
    for pair in climbing.windows(2) {
        assert!((pair[1].position.y - pair[0].position.y).abs() < 0.05);
        assert!(pair[1].position.z >= pair[0].position.z - 1e-4);
    }

    // On the deck it drives on to the north rim's kerb, level.
    let parked = runner.scene().world_transform(tank);
    assert!((parked.position.y - DECK_HEIGHT).abs() < 1e-3, "{parked:?}");
    assert!(
        parked.position.z > 47.0,
        "stopped short of the rim: {parked:?}"
    );
    assert!(
        parked.position.z < 48.5,
        "drove through the rim kerb: {parked:?}"
    );
    assert!(hull_pitch(&parked).abs() < 1e-3);
    assert!((parked.position.x - 20.0).abs() < 0.05);

    // Reversing takes it back down the ramp onto the ground.
    hold(&mut runner, Some(egui::Key::S), 6.0, 60);
    hold(&mut runner, None, 2.0, 60);
    let back = runner.scene().world_transform(tank);
    assert!(back.position.z < 32.0, "did not reverse down: {back:?}");
    assert!(back.position.y.abs() < 1e-3, "{back:?}");
    assert!(hull_pitch(&back).abs() < 1e-3);
}

#[test]
fn tank_climbs_the_west_ramp_from_the_middle_of_the_arena() {
    // West of the ramp's foot, facing east up it.
    let (mut runner, tank) =
        raised_area_runner(vec3(1.0, 0.0, 46.0), std::f32::consts::FRAC_PI_2, 60);
    hold(&mut runner, Some(egui::Key::W), 5.0, 60);
    hold(&mut runner, None, 2.0, 60);
    let parked = runner.scene().world_transform(tank);
    assert!((parked.position.y - DECK_HEIGHT).abs() < 1e-3, "{parked:?}");
    // Across the deck to the east rim's kerb.
    assert!(
        parked.position.x > 23.0 && parked.position.x < 24.0,
        "{parked:?}"
    );
    assert!((parked.position.z - 46.0).abs() < 0.05);
}

#[test]
fn ramp_climbs_are_the_same_at_30_and_144_fps() {
    let drive = |fps: usize| {
        let (mut runner, tank) = raised_area_runner(vec3(20.0, 0.0, 28.0), 0.0, fps);
        runner.press_key(egui::Key::W);
        runner.step_many(fps * 3 / 2);
        runner.scene().world_transform(tank).position
    };
    let slow = drive(30);
    let fast = drive(144);
    assert!(
        slow.y > 0.3 && slow.y < DECK_HEIGHT,
        "on the ramp: {slow:?}"
    );
    assert!(
        (slow - fast).magnitude() < 0.05,
        "30 fps: {slow:?}\n144 fps: {fast:?}"
    );
}

#[test]
fn ramp_kerbs_and_platform_sides_stop_tanks_on_the_ground() {
    // Driving east into the south ramp's west kerb, halfway up its side.
    let (mut runner, tank) =
        raised_area_runner(vec3(15.0, 0.0, 37.0), std::f32::consts::FRAC_PI_2, 60);
    hold(&mut runner, Some(egui::Key::W), 2.0, 60);
    let stopped = runner.scene().world_transform(tank).position;
    assert!(stopped.x > 16.0 && stopped.x < 16.8, "{stopped:?}");
    assert!(stopped.y.abs() < 1e-3, "{stopped:?}");
    // Driving north into the platform's south face, beside the ramp.
    let (mut runner, tank) = raised_area_runner(vec3(24.0, 0.0, 36.0), 0.0, 60);
    hold(&mut runner, Some(egui::Key::W), 2.0, 60);
    let stopped = runner.scene().world_transform(tank).position;
    assert!(stopped.z > 39.5 && stopped.z < 40.1, "{stopped:?}");
    assert!(stopped.y.abs() < 1e-3, "{stopped:?}");
}

/// Points the cursor at `point` through the arena camera as it now stands.
fn point_cursor_at(runner: &mut HeadlessSceneRunner, point: Vec3) {
    use calyx_engine::component::ComponentCamera;
    let scene = runner.scene();
    let camera = object_named(scene, "Camera");
    let viewport = nalgebra_glm::vec2(1280.0, 720.0);
    let lens = scene
        .read_component::<ComponentCamera, _, _>(camera, |c| c.camera(viewport.x / viewport.y))
        .unwrap();
    let clip = lens.projection
        * scene.world_transform(camera).inverse_matrix()
        * nalgebra_glm::vec4(point.x, point.y, point.z, 1.0);
    let ndc = clip.xyz() / clip.w;
    runner.move_pointer(egui::pos2(
        (ndc.x + 1.0) * 0.5 * viewport.x,
        (1.0 - ndc.y) * 0.5 * viewport.y,
    ));
}

/// Aims at `point` until the camera has settled on it.
fn aim_at(runner: &mut HeadlessSceneRunner, point: Vec3) {
    for _ in 0..6 {
        point_cursor_at(runner, point);
        runner.step_many(20);
    }
}

/// Fires until `target` loses health and returns where the first round was
/// first seen, and the round.
fn fire_until_hit(
    runner: &mut HeadlessSceneRunner,
    target: calyx_engine::scene::GameObject,
) -> (Vec3, sandbox::tank::ComponentProjectile) {
    let full_health = current_health(runner.scene(), target);
    runner.press_key(egui::Key::Space);
    let mut first_round = None;
    let mut steps = 0;
    while current_health(runner.scene(), target) == full_health {
        runner.step();
        if first_round.is_none() {
            let scene = runner.scene();
            first_round = find_named(scene, "Projectile").map(|round| {
                (
                    scene.world_transform(round).position,
                    scene
                        .read_component::<sandbox::tank::ComponentProjectile, _, _>(round, |p| *p)
                        .unwrap(),
                )
            });
        }
        steps += 1;
        assert!(steps < 180, "a round should hit the target");
    }
    runner.release_key(egui::Key::Space);
    first_round.expect("the tank should fire")
}

#[test]
fn tank_on_the_platform_hits_a_target_on_the_ground() {
    // On the deck's southeast, facing south over the rim kerb.
    let (mut runner, tank) =
        raised_area_runner(vec3(23.0, DECK_HEIGHT, 45.0), std::f32::consts::PI, 60);
    let target = object_named(runner.scene(), "Target A");
    let mut pose = runner.scene().world_transform(target);
    let ground = vec3(23.0, 0.0, 34.0);
    pose.position = vec3(ground.x, pose.position.y, ground.z);
    runner
        .scene_mut()
        .set_world_transform(target, pose.matrix());
    aim_at(&mut runner, ground);

    let crosshair = object_named(runner.scene(), "Crosshair");
    let aimed = runner.scene().world_transform(crosshair).position;
    assert!(
        (aimed - ground).magnitude() < 0.3,
        "the crosshair is on the ground: {aimed:?}"
    );
    assert!((runner.scene().world_transform(tank).position.y - DECK_HEIGHT).abs() < 1e-3);

    let (start, round) = fire_until_hit(&mut runner, target);
    assert!(
        start.y > DECK_HEIGHT + 1.0,
        "fired from the deck: {start:?}"
    );
    assert!(
        round.direction.y < -0.05,
        "the round dives: {:?}",
        round.direction
    );
}

#[test]
fn tank_on_the_ground_hits_a_target_on_the_platform() {
    let (mut runner, _tank) = raised_area_runner(vec3(21.0, 0.0, 30.0), 0.0, 60);
    let target = object_named(runner.scene(), "Target A");
    let mut pose = runner.scene().world_transform(target);
    let deck = vec3(21.0, DECK_HEIGHT, 46.0);
    pose.position = deck + vec3(0.0, pose.position.y, 0.0);
    runner
        .scene_mut()
        .set_world_transform(target, pose.matrix());
    aim_at(&mut runner, deck);

    let (start, round) = fire_until_hit(&mut runner, target);
    assert!(
        start.y < DECK_HEIGHT + 0.5,
        "fired from the ground: {start:?}"
    );
    assert!(
        round.direction.y > 0.05,
        "the round climbs: {:?}",
        round.direction
    );
}

#[test]
fn platform_and_ramps_stop_level_shells_and_deck_cover_stops_shells_from_the_deck() {
    use calyx_engine::component::ColliderShape;
    use rapier3d::pipeline::QueryFilter;
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    let shell_hits = |origin: Vec3, direction: Vec3, length: f32| {
        runner
            .scene()
            .physics
            .cast_shape(
                ColliderShape::Sphere { radius: 0.18 },
                origin,
                nalgebra::UnitQuaternion::identity(),
                direction * length,
                1.0,
                QueryFilter::only_fixed(),
            )
            .is_some()
    };
    let gun = 1.6;
    // A level round at gun height from the ground meets the platform's
    // side, and the crest of a ramp, rather than flying over them.
    assert!(shell_hits(vec3(24.0, gun, 36.0), vec3(0.0, 0.0, 1.0), 6.0));
    assert!(shell_hits(vec3(12.0, gun, 40.5), vec3(1.0, 0.0, 0.0), 10.0));
    // From the deck, at gun height above it, the deck's sandbags stop a
    // round crossing them, as cover does on the ground.
    assert!(shell_hits(
        vec3(17.0, DECK_HEIGHT + gun, 44.5),
        vec3(0.0, 0.0, -1.0),
        2.0
    ));
    // Over the low end of a ramp, a level round flies on: a ramp is a
    // slope, not cover.
    assert!(!shell_hits(vec3(16.0, gun, 34.0), vec3(1.0, 0.0, 0.0), 8.0));
}

/// The side's sight the renderer's fog of war draws this frame.
fn fog_sight(scene: &Scene) -> calyx_engine::component::FogOfWarSight {
    use calyx_engine::component::ComponentFogOfWar;
    scene
        .objects()
        .find_map(|object| {
            scene
                .read_component::<ComponentFogOfWar, _, _>(object, |fog| fog.sight.clone())
                .flatten()
        })
        .expect("the arena's fog of war shows the side's sight")
}

// The interior wall in the east middle: a run along Z at world x = 10, from
// z = 32.4 to 39.6, 1.2 m thick.
#[test]
fn interior_walls_stop_rounds_and_block_sight() {
    let ground = |p: Vec3| nalgebra_glm::vec2(p.x, p.z);
    // West of the wall, facing east across it.
    let (mut runner, tank) =
        raised_area_runner(vec3(4.0, 0.0, 36.0), std::f32::consts::FRAC_PI_2, 60);
    let target = object_named(runner.scene(), "Target A");
    let place = |runner: &mut HeadlessSceneRunner, x: f32, z: f32| {
        let mut pose = runner.scene().world_transform(target);
        pose.position = vec3(x, pose.position.y, z);
        runner
            .scene_mut()
            .set_world_transform(target, pose.matrix());
    };
    // Behind the wall, well within the tank's vision range.
    place(&mut runner, 15.0, 36.0);
    hold(&mut runner, None, 1.0, 60);
    let behind = runner.scene().world_transform(target).position;
    let from = ground(runner.scene().world_transform(tank).position);
    assert!(
        !runner.scene().is_visible(target),
        "fog hides the target behind the wall"
    );
    // The wall casts a fog shadow over the ground behind it, and only
    // behind it: the ground the same distance away past its end is lit.
    let sight = fog_sight(runner.scene());
    assert!(!sight.clear_line(from, ground(behind)));
    assert!(sight.clear_line(from, nalgebra_glm::vec2(12.0, 28.0)));

    // Rounds fired at the target stop at the wall's near face.
    aim_at(&mut runner, vec3(behind.x, 0.0, behind.z));
    let full_health = current_health(runner.scene(), target);
    runner.press_key(egui::Key::Space);
    let mut rounds = 0;
    let mut farthest = f32::NEG_INFINITY;
    for _ in 0..120 {
        runner.step();
        let scene = runner.scene();
        for object in scene.objects() {
            if scene
                .read_component::<sandbox::tank::ComponentProjectile, _, _>(object, |_| ())
                .is_some()
            {
                rounds += 1;
                farthest = farthest.max(scene.world_transform(object).position.x);
            }
        }
    }
    runner.release_key(egui::Key::Space);
    assert!(rounds > 0, "the tank fired");
    assert!(farthest < 9.4, "a round reached x = {farthest}");
    assert_eq!(current_health(runner.scene(), target), full_health);

    // Past the wall's end, the same tank sees the target and hits it.
    place(&mut runner, 12.0, 28.0);
    hold(&mut runner, None, 1.0, 60);
    assert!(runner.scene().is_visible(target), "the target is in sight");
    aim_at(&mut runner, vec3(12.0, 0.0, 28.0));
    fire_until_hit(&mut runner, target);
}

/// The ground footprint of a box collider: the convex hull, counterclockwise,
/// of its eight corners seen from above.
fn footprint(transform: &Transform, half_extents: Vec3) -> Vec<nalgebra_glm::Vec2> {
    let mut points: Vec<_> = (0..8)
        .map(|i| {
            let sign = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
            let corner = transform.transform_position(&vec3(
                sign(1) * half_extents.x,
                sign(2) * half_extents.y,
                sign(4) * half_extents.z,
            ));
            nalgebra_glm::vec2(corner.x, corner.z)
        })
        .collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: nalgebra_glm::Vec2, a: nalgebra_glm::Vec2, b: nalgebra_glm::Vec2| {
        (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
    };
    let mut hull: Vec<nalgebra_glm::Vec2> = Vec::new();
    for pass in [false, true] {
        let start = hull.len();
        let ordered: Vec<_> = if pass {
            points.iter().rev().copied().collect()
        } else {
            points.clone()
        };
        for point in ordered {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0
            {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// Distance from `point` to a convex counterclockwise `hull`, 0 inside it.
fn distance_to_hull(point: nalgebra_glm::Vec2, hull: &[nalgebra_glm::Vec2]) -> f32 {
    let mut inside = true;
    let mut nearest = f32::INFINITY;
    for (i, &a) in hull.iter().enumerate() {
        let b = hull[(i + 1) % hull.len()];
        let edge = b - a;
        if edge.x * (point.y - a.y) - edge.y * (point.x - a.x) < 0.0 {
            inside = false;
        }
        let t = ((point - a).dot(&edge) / edge.norm_squared()).clamp(0.0, 1.0);
        nearest = nearest.min((a + edge * t - point).norm());
    }
    if inside {
        0.0
    } else {
        nearest
    }
}

/// With the interior walls among the ramps, the platform and the cover,
/// every patch of ground a tank fits on is one drive from the spawn: no
/// wall closes off a pocket a tank could reach some other way, or strands
/// it, and the feet of both ramps and every unit stay reachable.
#[test]
fn the_whole_pen_is_one_drive_from_the_spawn() {
    use calyx_engine::component::{ColliderShape, ComponentCollider};
    use sandbox::terrain::ComponentDriveSurface;
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let arena = object_named(&scene, "Arena");
    // Everything in the pen that stops a tank on the ground: every solid
    // but the drive surfaces, which tanks ride.
    let obstacles: Vec<_> = scene
        .descendants(arena)
        .filter(|&object| {
            scene
                .read_component::<ComponentDriveSurface, _, _>(object, |_| ())
                .is_none()
        })
        .filter_map(|object| {
            let shape = scene
                .read_component::<ComponentCollider, _, _>(object, |collider| collider.shape)?;
            let ColliderShape::Cuboid { half_extents } = shape else {
                panic!("{} is not a box", scene.name(object));
            };
            Some(footprint(&scene.world_transform(object), half_extents))
        })
        .collect();
    assert!(
        scene.descendants(arena).any(|object| scene
            .name(object)
            .starts_with("Arena Wall West Middle collision")),
        "the arena has interior walls"
    );
    // A tank fits where its hull's half width clears every obstacle.
    let clearance = 1.2;
    let step = 0.25;
    let (min_x, max_x, min_z, max_z) = (-28.4, 28.4, -8.4, 52.4);
    let columns = ((max_x - min_x) / step) as usize;
    let rows = ((max_z - min_z) / step) as usize;
    let cell = |c: usize, r: usize| {
        nalgebra_glm::vec2(
            min_x + (c as f32 + 0.5) * step,
            min_z + (r as f32 + 0.5) * step,
        )
    };
    let free: Vec<bool> = (0..rows)
        .flat_map(|r| (0..columns).map(move |c| (c, r)))
        .map(|(c, r)| {
            obstacles
                .iter()
                .all(|hull| distance_to_hull(cell(c, r), hull) > clearance)
        })
        .collect();
    let index = |p: nalgebra_glm::Vec2| {
        let c = ((p.x - min_x) / step) as usize;
        let r = ((p.y - min_z) / step) as usize;
        r * columns + c
    };
    let spawn = scene
        .world_transform(object_named(&scene, "Player Spawn"))
        .position;
    let mut reached = vec![false; free.len()];
    let mut queue = vec![index(nalgebra_glm::vec2(spawn.x, spawn.z))];
    assert!(free[queue[0]], "a tank fits at the spawn");
    reached[queue[0]] = true;
    while let Some(i) = queue.pop() {
        let (c, r) = (i % columns, i / columns);
        let neighbours = [
            (c > 0).then(|| i - 1),
            (c + 1 < columns).then(|| i + 1),
            (r > 0).then(|| i - columns),
            (r + 1 < rows).then(|| i + columns),
        ];
        for next in neighbours.into_iter().flatten() {
            if free[next] && !reached[next] {
                reached[next] = true;
                queue.push(next);
            }
        }
    }
    let stranded: Vec<_> = (0..free.len())
        .filter(|&i| free[i] && !reached[i])
        .map(|i| cell(i % columns, i / columns))
        .collect();
    assert!(
        stranded.is_empty(),
        "{} cells a tank fits on are cut off, the first at {:?}",
        stranded.len(),
        stranded.first()
    );
    // The feet of both ramps, and every unit.
    for (name, x, z) in [("south ramp", 20.0, 32.0), ("west ramp", 6.0, 46.0)] {
        assert!(
            reached[index(nalgebra_glm::vec2(x, z))],
            "the {name}'s foot"
        );
    }
    for name in ["Target A", "Target B", "Sentry"] {
        let p = scene.world_transform(object_named(&scene, name)).position;
        assert!(reached[index(nalgebra_glm::vec2(p.x, p.z))], "{name}");
    }
}

#[test]
fn tanks_props_and_terrain_draw_on_their_own_outline_layers_in_the_same_ink() {
    use calyx_engine::component::{ComponentMesh, ComponentOutline};
    use calyx_model_gen::palette::{PROP_OUTLINE, TANK_OUTLINE, TERRAIN_OUTLINE};
    let (copy, assets) = sandbox_assets();
    let scene = load_sandbox_scene(&copy, &assets);
    let registries = assets.registries.lock_read();
    // The outline styles of the meshes under `root`, ignoring style 0: the
    // ground, deck paving and signals draw no ink.
    let styles = |root| {
        scene
            .descendants(root)
            .filter_map(|object| {
                scene.read_component::<ComponentMesh, _, _>(object, |mesh| {
                    mesh.material
                        .get_ref(&registries)
                        .expect("every arena material loads")
                        .read()
                        .outline_style()
                })
            })
            .filter(|style| *style != 0)
            .collect::<std::collections::BTreeSet<u32>>()
    };
    let one = |style| std::collections::BTreeSet::from([style]);
    assert_eq!(styles(object_named(&scene, "Player")), one(TANK_OUTLINE));
    assert_eq!(styles(object_named(&scene, "Sentry")), one(PROP_OUTLINE));
    let arena = object_named(&scene, "Arena");
    let (mut terrain, mut props) = (0, 0);
    for child in scene.children(arena) {
        let name = scene.name(child);
        let expected = if ["Arena Wall", "Arena Platform", "Arena Ramp"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            terrain += 1;
            TERRAIN_OUTLINE
        } else if ["Arena Cover", "Arena Prop", "Arena Grass"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            props += 1;
            PROP_OUTLINE
        } else {
            continue;
        };
        let found = styles(child);
        assert!(
            found.is_empty() || found == one(expected),
            "{name} draws outline styles {found:?}, not {expected}"
        );
    }
    assert!(
        terrain > 50 && props > 30,
        "{terrain} terrain, {props} props"
    );

    // Every layer is drawn in the arena's one ink.
    let outline = scene
        .objects()
        .find_map(|object| scene.read_component::<ComponentOutline, _, _>(object, Clone::clone))
        .expect("the arena has an Outline effect");
    let layers = [TERRAIN_OUTLINE, TANK_OUTLINE, PROP_OUTLINE];
    assert_eq!(outline.styles.len(), layers.len());
    for style in &outline.styles {
        assert_eq!(style, &outline.styles[0]);
    }
}

/// The farmland's creek stops tanks at its bank, but rounds fly over it and
/// sight crosses it: a tank driving south at it from the field west of the
/// lane stops short of the water, then sees and hits a target on the far
/// bank.
#[test]
fn the_creek_stops_tanks_but_not_rounds_or_sight() {
    // North of the creek, facing south across it; its north shore here is
    // at about z = 4.8.
    let (mut runner, tank) = raised_area_runner(vec3(-26.0, 0.0, 10.0), std::f32::consts::PI, 60);
    hold(&mut runner, Some(egui::Key::W), 3.0, 60);
    let stopped = runner.scene().world_transform(tank).position;
    assert!(
        stopped.z < 8.0,
        "the tank drove towards the creek: {stopped:?}"
    );
    assert!(
        stopped.z > 4.8,
        "the tank drove into the creek: {stopped:?}"
    );
    // On the far bank, south of the water.
    let target = object_named(runner.scene(), "Target A");
    let mut pose = runner.scene().world_transform(target);
    pose.position = vec3(-25.0, pose.position.y, -4.5);
    runner
        .scene_mut()
        .set_world_transform(target, pose.matrix());
    hold(&mut runner, None, 1.0, 60);
    assert!(runner.scene().is_visible(target), "sight crosses the creek");
    aim_at(&mut runner, vec3(-25.0, 0.0, -4.5));
    fire_until_hit(&mut runner, target);
}

/// The bridge carries the west sand lane over the creek: a tank at the
/// lane's south end, past the creek, drives north over it to the field.
#[test]
fn tanks_cross_the_creek_on_the_bridge() {
    let end = drive_from(vec3(-20.0, 0.0, -5.0), 0.0, 60, 3.0);
    assert!(
        end.z > 8.0,
        "the tank stopped at {end:?}, short of the far bank"
    );
    assert!((end.x + 20.0).abs() < 0.1, "unexpected deflection: {end:?}");
}
