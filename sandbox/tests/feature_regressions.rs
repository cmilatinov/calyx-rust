use engine::context::GameContext;
use engine::math::Transform;
use engine::scene::Scene;
use engine::test_support::{test_asset_context_with_assets, HeadlessSceneRunner};
use nalgebra_glm::Vec3;
use std::path::PathBuf;

fn sandbox_assets() -> engine::context::AssetContext {
    let assets = test_asset_context_with_assets(vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets"),
    ]);
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    assets
}

fn load_sandbox_scene(assets: &engine::context::AssetContext) -> Scene {
    assets
        .registries
        .assets
        .read()
        .reload_by_path::<Scene>(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join("scene.cxscene"),
        )
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
    let assets = sandbox_assets();
    let scene = load_sandbox_scene(&assets);
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
    let assets = sandbox_assets();
    let scene = load_sandbox_scene(&assets);
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

#[test]
fn firing_projectiles_does_not_move_static_targets() {
    let assets = sandbox_assets();
    let scene = load_sandbox_scene(&assets);
    let target_transforms = ["Target A", "Target B"].map(|name| {
        let target = object_named(&scene, name);
        (name, scene.world_transform(target))
    });
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    runner.press_key(egui::Key::Space);
    runner.step_many(120);

    for (name, expected_transform) in target_transforms {
        let target = object_named(runner.scene(), name);
        assert_transform_eq(runner.scene().world_transform(target), expected_transform);
    }
}
