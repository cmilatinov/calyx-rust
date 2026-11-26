use super::{
    assembly_artifact_path, assembly_build_target_dir_from_exe, assembly_target_dir,
    cargo_build_args, copy_runtime_dependencies, is_rust_runtime_library, mark_built_for, predates,
    remove_stale_shadow_copies, scene_paths_match, shadow_copy_folder_name, AssemblyBuildOutcome,
    AssemblyBuildQueue, AssemblyLoad, ProjectAssemblyStatus, ShadowCopyFolder,
    FOREIGN_SHADOW_COPY_GRACE_MS,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn an_assembly_built_before_the_editor_predates_it() {
    let dir = std::env::temp_dir().join(format!("calyx_assembly_predates_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (assembly, editor) = (dir.join("project.dll"), dir.join("editor.exe"));
    let now = SystemTime::now();
    let touch = |path: &Path, time: SystemTime| {
        fs::write(path, b"").unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    };

    touch(&assembly, now - Duration::from_secs(60));
    touch(&editor, now);
    assert!(predates(&assembly, &editor));

    touch(&assembly, now + Duration::from_secs(60));
    assert!(!predates(&assembly, &editor));
    assert!(!predates(&dir.join("missing.dll"), &editor));

    // A build that found the older assembly fresh marks it as built for
    // this editor, so the next startup loads it.
    touch(&assembly, now - Duration::from_secs(60));
    mark_built_for(&assembly, &editor);
    assert!(!predates(&assembly, &editor));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn project_assembly_status_defaults_to_unloaded() {
    assert!(!ProjectAssemblyStatus::default().is_loaded());
}

#[test]
fn dev_profile_uses_default_cargo_build() {
    assert_eq!(
        cargo_build_args("dev", Path::new("target")),
        [
            "build",
            "--lib",
            "--target-dir",
            "target",
            "--config",
            "profile.dev.build-override.debug=true"
        ]
    );
    assert_eq!(assembly_target_dir("dev"), "debug");
}

#[test]
fn custom_profile_is_forwarded_to_cargo_and_target_dir() {
    assert_eq!(
        cargo_build_args("release-with-debug", Path::new("target")),
        [
            "build",
            "--lib",
            "--target-dir",
            "target",
            "--profile",
            "release-with-debug"
        ]
    );
    assert_eq!(
        assembly_target_dir("release-with-debug"),
        "release-with-debug"
    );
}

#[test]
fn assembly_paths_use_isolated_target_dir() {
    let target = PathBuf::from("custom-output");
    for executable in [
        "custom-output/debug/editor",
        "custom-output/debug/deps/editor-hash",
        "custom-output/debug/build/editor/hash/out/editor-hash",
        "custom-output/release-with-debug/editor",
        "custom-output/release-with-debug/build/editor/hash/out/editor-hash",
    ] {
        assert_eq!(
            assembly_build_target_dir_from_exe(Path::new(executable)),
            target,
            "incorrect target directory for {executable}"
        );
    }
    assert_eq!(
        assembly_artifact_path(&target, "dev", "sandbox"),
        target
            .join("debug")
            .join(calyx_engine::utils::lib_file_name("sandbox"))
    );
}

#[test]
fn runtime_dependencies_are_copied_next_to_shadow_assembly() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("calyx-assembly-copy-{suffix}"));
    let source_dir = root.join("source");
    let target_dir = root.join("target");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();

    let source = source_dir.join(calyx_engine::utils::lib_file_name("sandbox"));
    let dependency = source_dir.join(calyx_engine::utils::lib_file_name("calyx_engine"));
    fs::write(&source, b"sandbox").unwrap();
    fs::write(&dependency, b"calyx_engine").unwrap();

    copy_runtime_dependencies(&source, &target_dir).unwrap();

    assert!(target_dir
        .join(calyx_engine::utils::lib_file_name("calyx_engine"))
        .exists());
    assert!(!target_dir
        .join(calyx_engine::utils::lib_file_name("sandbox"))
        .exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rust_runtime_library_detection_matches_std_only() {
    #[cfg(windows)]
    assert!(is_rust_runtime_library(Path::new(
        "std-00eb2f7586512494.dll"
    )));
    #[cfg(unix)]
    assert!(is_rust_runtime_library(Path::new(
        "libstd-00eb2f7586512494.so"
    )));
    #[cfg(windows)]
    assert!(!is_rust_runtime_library(Path::new(
        "rustc_driver-1815a83be396bd1c.dll"
    )));
    #[cfg(unix)]
    assert!(!is_rust_runtime_library(Path::new(
        "librustc_driver-1815a83be396bd1c.so"
    )));
}

#[test]
fn configured_scene_path_matches_the_open_scene_after_canonicalization() {
    let root = std::env::temp_dir().join(format!(
        "calyx-project-manager-path-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let scene = root.join("scene.cxscene");
    fs::write(&scene, "{}").unwrap();

    assert!(scene_paths_match(
        Some(&scene),
        &root.join(".").join("scene.cxscene")
    ));
    assert!(!scene_paths_match(
        Some(&scene),
        &root.join("other.cxscene")
    ));

    fs::remove_dir_all(root).unwrap();
}

/// Editor play runs the scene open in the editor, not the startup scene
/// a project's `project.toml` names for the standalone player.
#[test]
fn editor_play_runs_the_open_scene_rather_than_the_startup_scene() {
    use super::ProjectManager;
    use crate::scene_file::write_scene_file;
    use calyx_engine::component::ComponentID;
    use calyx_engine::context::GameContext;
    use calyx_engine::core::Ref;
    use calyx_engine::input::Input;
    use calyx_engine::test_support::{test_asset_context_with_assets, test_scene};

    let root = std::env::temp_dir().join(format!(
        "calyx-project-startup-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let levels = root.join("assets").join("levels");
    fs::create_dir_all(&levels).unwrap();
    let root = dunce::canonicalize(root).unwrap();
    fs::write(
        root.join("project.toml"),
        "name = \"startup-project\"
             creation_date = \"2026-01-01T00:00:00Z\"
             startup_scene = \"assets/levels/first.cxscene\"
             [runtime.window]
width = 800
height = 600
",
    )
    .unwrap();
    let mut startup = test_scene();
    startup.create(
        Some(ComponentID {
            name: "Configured startup object".into(),
            ..Default::default()
        }),
        None,
    );
    let startup_path = root.join("assets").join("levels").join("first.cxscene");
    write_scene_file(&startup_path, &startup).unwrap();
    let mut other = test_scene();
    other.create(
        Some(ComponentID {
            name: "Named scene object".into(),
            ..Default::default()
        }),
        None,
    );
    let other_path = root.join("assets").join("levels").join("other.cxscene");
    write_scene_file(&other_path, &other).unwrap();

    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.join("assets")]));
    game.scenes.current_scene_mut().create(
        Some(ComponentID {
            name: "Authoring object".into(),
            ..Default::default()
        }),
        None,
    );
    let project = ProjectManager::new(
        game.assets.clone(),
        &root,
        game.resources.background().clone(),
        Ref::new(ProjectAssemblyStatus::default()),
    )
    .unwrap();
    assert_eq!(
        project.read().runtime_settings().window.non_zero_size(),
        Some((800, 600))
    );

    let started = project
        .read()
        .start_simulation(&mut game.scenes, None)
        .unwrap();
    game.scenes.start_simulation();

    assert_eq!(started, None);
    assert!(game.scenes.is_simulating());
    let names = |scene: &calyx_engine::scene::Scene| {
        scene
            .objects()
            .map(|object| scene.name(object))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(game.scenes.simulation_scene()), ["Authoring object"]);
    assert_eq!(names(game.scenes.current_scene()), ["Authoring object"]);

    // The open scene runs a frame like any other simulation.
    let input_context = egui::Context::default();
    let input = Input::from_ctx(&input_context, None, game.resources.input_mut(), false);
    let assets = game.assets.lock_read();
    game.scenes.prepare();
    game.scenes
        .update(&assets.registries, &mut game.resources, &input);
    drop(assets);
    game.scenes.stop_simulation();
    assert!(!game.scenes.is_simulating());

    // A scene named for the run replaces the open scene.
    let started = project
        .read()
        .start_simulation(&mut game.scenes, Some(other_path.clone()))
        .unwrap();
    assert_eq!(started.as_deref(), Some(other_path.as_path()));
    assert_eq!(
        names(game.scenes.simulation_scene()),
        ["Named scene object"]
    );
    game.scenes.stop_simulation();

    drop((project, game));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn stale_shadow_copies_are_removed_and_the_current_one_is_kept() {
    let root = std::env::temp_dir().join(format!(
        "calyx-shadow-cleanup-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let now: u128 = 10_000_000_000;
    let ancient = now - FOREIGN_SHADOW_COPY_GRACE_MS - 1;
    let folder = |project: &str, process_id: u32, timestamp: u128| {
        root.join(shadow_copy_folder_name(project, process_id, timestamp))
    };
    let own_older = folder("sandbox", 7, now - 1_000);
    let own_other_project = folder("other_game", 7, now - 500);
    let current = folder("sandbox", 7, now);
    let own_pending = folder("sandbox", 7, now + 1_000);
    let foreign_recent = folder("sandbox", 8, now - 1_000);
    let foreign_ancient = folder("sandbox", 8, ancient);
    let legacy_recent = root.join(format!("sandbox_{}", now - 1_000));
    let legacy_ancient = root.join(format!("sandbox_{ancient}"));
    let unrelated = root.join("notes");
    for dir in [
        &own_older,
        &own_other_project,
        &current,
        &own_pending,
        &foreign_recent,
        &foreign_ancient,
        &legacy_recent,
        &legacy_ancient,
        &unrelated,
    ] {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(calyx_engine::utils::lib_file_name("sandbox")),
            b"lib",
        )
        .unwrap();
    }

    remove_stale_shadow_copies(&current, 7);

    assert!(!own_older.exists());
    assert!(!own_other_project.exists());
    assert!(current
        .join(calyx_engine::utils::lib_file_name("sandbox"))
        .exists());
    assert!(own_pending.exists(), "a newer, not yet loaded copy is kept");
    assert!(
        foreign_recent.exists(),
        "another editor's recent copy may still be queued there"
    );
    assert!(legacy_recent.exists());
    assert!(!foreign_ancient.exists());
    assert!(!legacy_ancient.exists());
    assert!(
        unrelated.exists(),
        "folders that are not shadow copies are kept"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shadow_copy_folder_name_round_trips() {
    assert_eq!(
        ShadowCopyFolder::parse(Path::new(&shadow_copy_folder_name(
            "my_game",
            42,
            1_700_000_000_000
        ))),
        Some(ShadowCopyFolder {
            process_id: Some(42),
            timestamp: 1_700_000_000_000
        })
    );
    assert_eq!(
        ShadowCopyFolder::parse(Path::new("my_game_1700000000000")),
        Some(ShadowCopyFolder {
            process_id: None,
            timestamp: 1_700_000_000_000
        })
    );
    assert_eq!(ShadowCopyFolder::parse(Path::new("my_game")), None);
    assert_eq!(ShadowCopyFolder::parse(Path::new("notes")), None);
    assert_eq!(ShadowCopyFolder::parse(Path::new("_123")), None);
}

#[test]
fn completed_build_is_loaded_once_on_the_polling_thread() {
    let queue = AssemblyBuildQueue::new();
    assert_eq!(queue.next_load(false), None);

    let built = Path::new("built.dll").to_path_buf();
    let reporter = queue.reporter();
    std::thread::spawn(move || reporter.send(AssemblyBuildOutcome::Built(built)).unwrap())
        .join()
        .unwrap();

    assert_eq!(
        queue.next_load(true),
        Some(AssemblyLoad::Snapshot(Path::new("built.dll").to_path_buf()))
    );
    assert_eq!(queue.next_load(true), None);
}

#[test]
fn each_completed_build_is_loaded_once_in_order() {
    let queue = AssemblyBuildQueue::new();
    let reporter = queue.reporter();
    reporter
        .send(AssemblyBuildOutcome::Built(
            Path::new("first.dll").to_path_buf(),
        ))
        .unwrap();
    reporter
        .send(AssemblyBuildOutcome::Built(
            Path::new("second.dll").to_path_buf(),
        ))
        .unwrap();

    assert_eq!(
        queue.next_load(true),
        Some(AssemblyLoad::Snapshot(Path::new("first.dll").to_path_buf()))
    );
    assert_eq!(
        queue.next_load(true),
        Some(AssemblyLoad::Snapshot(
            Path::new("second.dll").to_path_buf()
        ))
    );
    assert_eq!(queue.next_load(true), None);
}

#[test]
fn failed_build_keeps_the_loaded_assembly() {
    let root = std::env::temp_dir().join(format!(
        "calyx-assembly-failed-build-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let existing = root.join(calyx_engine::utils::lib_file_name("sandbox"));
    fs::write(&existing, b"sandbox").unwrap();
    let queue = AssemblyBuildQueue::new();

    queue
        .reporter()
        .send(AssemblyBuildOutcome::Failed(existing.clone()))
        .unwrap();
    assert_eq!(queue.next_load(true), None);

    queue
        .reporter()
        .send(AssemblyBuildOutcome::Failed(existing.clone()))
        .unwrap();
    assert_eq!(
        queue.next_load(false),
        Some(AssemblyLoad::Source(existing.clone()))
    );

    queue
        .reporter()
        .send(AssemblyBuildOutcome::Failed(root.join("missing.dll")))
        .unwrap();
    assert_eq!(queue.next_load(false), None);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_build_never_falls_back_to_an_assembly_older_than_the_editor() {
    let root = std::env::temp_dir().join(format!(
        "calyx-assembly-failed-stale-build-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let stale = root.join(calyx_engine::utils::lib_file_name("sandbox"));
    fs::write(&stale, b"sandbox").unwrap();
    // Older than the running test binary, which stands in for the editor.
    fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    let queue = AssemblyBuildQueue::new();

    queue
        .reporter()
        .send(AssemblyBuildOutcome::Failed(stale))
        .unwrap();
    assert_eq!(queue.next_load(false), None);

    fs::remove_dir_all(root).unwrap();
}

/// Reloads a real project assembly.
mod assembly_reload;
