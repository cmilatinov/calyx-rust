use super::super::{
    assembly_artifact_path, assembly_build_target_dir, assembly_profile, build_assembly,
    cargo_build_args, AssemblyBuildOutcome, ProjectAssemblyStatus, ProjectManager,
};
use calyx_engine::core::Ref;
use calyx_engine::input::Input;
use calyx_engine::scene::Scene;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The sandbox's tank controller, a project component with an update hook.
const TANK_CONTROLLER: &str = "21b467c0-9409-4a7f-b750-418809609eb1";

/// Builds the sandbox assembly the way the editor does and returns its
/// library.
fn build_sandbox_assembly() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox");
    let target_dir = assembly_build_target_dir();
    let profile = assembly_profile();
    let source = assembly_artifact_path(&target_dir, profile, "sandbox");
    match build_assembly(
        &root,
        profile,
        &cargo_build_args(profile, &target_dir),
        source,
    ) {
        AssemblyBuildOutcome::Built(source) => source,
        AssemblyBuildOutcome::Failed(source) => {
            panic!(
                "the sandbox assembly failed to build at {}",
                source.display()
            )
        }
    }
}

#[test]
fn a_reloaded_assembly_leaves_open_scenes_usable() {
    let source = build_sandbox_assembly();
    let mut game = calyx_engine::test_support::test_game_context();
    let project = ProjectManager::new(
        game.assets.clone(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sandbox"),
        game.resources.background().clone(),
        Ref::new(ProjectAssemblyStatus::default()),
    )
    .unwrap();
    assert!(project.write().load_assembly(&source, &mut game.scenes));

    let tank_controller = Uuid::parse_str(TANK_CONTROLLER).unwrap();
    let scene = game.scenes.current_scene_mut();
    let tank = scene.create(None, None);
    scene.bind_component_dyn(tank, tank_controller);
    let tank = scene.uuid(tank);
    let cached = game.scenes.current_scene().clone();
    let cached = game
        .assets
        .registries
        .assets
        .read()
        .create("reloaded_scene".into(), cached)
        .unwrap();
    game.scenes.start_simulation();

    // A fresh shadow copy is another library, so loading it unloads the
    // one that created the scenes' project components, as a rebuild does.
    assert!(project.write().load_assembly(&source, &mut game.scenes));

    // Save, play and close the scene.
    let holds_tank = |scene: &Scene| {
        let saved = serde_json::to_value(scene.snapshot()).unwrap();
        saved["components"][tank.to_string()]
            .get(TANK_CONTROLLER)
            .is_some()
    };
    assert!(holds_tank(game.scenes.current_scene()));
    assert!(holds_tank(&cached.read()));
    assert!(!game.scenes.has_simulation_scene());
    game.scenes.start_simulation();
    let input_context = egui::Context::default();
    let input = Input::from_ctx(&input_context, None, game.resources.input_mut(), false);
    let assets = game.assets.lock_read();
    game.scenes.prepare();
    game.scenes
        .update(&assets.registries, &mut game.resources, &input);
    game.scenes.stop_simulation();
    game.scenes.load_empty_scene();

    // The cached scene still holds project components, so it must go
    // before the project manager unloads the assembly.
    drop((assets, cached, game));
}
