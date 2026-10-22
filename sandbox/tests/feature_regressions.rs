use engine::context::GameContext;
use engine::math::Transform;
use engine::scene::Scene;
use engine::test_support::{HeadlessSceneRunner, TestAssets};
use nalgebra_glm::Vec3;
use sandbox::tank::{ComponentProjectileTarget, ComponentTankController};

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

fn hit_count(scene: &Scene, target: engine::scene::GameObject) -> u32 {
    scene
        .read_component::<ComponentProjectileTarget, _, _>(target, |target| target.hit_count)
        .expect("sandbox targets should be projectile targets")
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

#[test]
fn firing_projectiles_hits_targets_without_moving_them() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_sandbox_scene(&copy, &assets);
    let aimed_target = object_named(&scene, "Target A");
    // Close enough that the arena's centre cover, whose near face is 7 m
    // ahead of the spawn and now absorbs rounds, is not in the way.
    place_on_firing_line(&mut scene, aimed_target, 4.0);
    let target_transforms = ["Target A", "Target B"].map(|name| {
        let target = object_named(&scene, name);
        (name, scene.world_transform(target))
    });
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    runner.step_many(120);

    let aimed_target = object_named(runner.scene(), "Target A");
    assert!(
        hit_count(runner.scene(), aimed_target) > 0,
        "a projectile should hit the target on the firing line"
    );
    for (name, expected_transform) in target_transforms {
        let target = object_named(runner.scene(), name);
        assert_transform_eq(runner.scene().world_transform(target), expected_transform);
    }
}
