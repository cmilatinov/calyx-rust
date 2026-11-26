use super::{
    native_options, player_icon, player_scene, present_mode, profile_file, renderer_options,
    window_pixels, ProjectRuntimeSettings,
};
use egui_wgpu::wgpu;
use std::ffi::OsStr;
use std::path::PathBuf;

fn project_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sandbox")
}

#[test]
fn player_opens_the_scene_the_environment_names_over_the_startup_scene() {
    let settings = ProjectRuntimeSettings::load(project_directory()).unwrap();
    assert_eq!(
        player_scene(&settings, None),
        settings.startup_scene_path(),
        "without a scene named, the startup scene"
    );
    assert_eq!(
        player_scene(&settings, Some(OsStr::new(""))),
        settings.startup_scene_path()
    );
    assert_eq!(
        player_scene(
            &settings,
            Some(OsStr::new("assets/game/scenes/arena.cxscene"))
        ),
        Some(project_directory().join("assets/game/scenes/arena.cxscene"))
    );
}

#[test]
fn player_appends_profiles_to_the_file_the_environment_names() {
    assert_eq!(profile_file(None), None);
    assert_eq!(profile_file(Some(OsStr::new("1"))), None);
    assert_eq!(profile_file(Some(OsStr::new(""))), None);
    assert_eq!(
        profile_file(Some(OsStr::new("logs/profile.jsonl"))),
        Some(PathBuf::from("logs/profile.jsonl"))
    );
}

#[test]
fn player_waits_for_vsync_unless_the_environment_turns_it_off() {
    assert_eq!(present_mode(None), wgpu::PresentMode::AutoVsync);
    assert_eq!(
        present_mode(Some(OsStr::new("1"))),
        wgpu::PresentMode::AutoVsync
    );
    assert_eq!(
        present_mode(Some(OsStr::new("0"))),
        wgpu::PresentMode::AutoNoVsync
    );
}

#[test]
fn player_sizes_its_window_in_pixels_the_environment_names() {
    assert_eq!(window_pixels(None), None);
    assert_eq!(
        window_pixels(Some(OsStr::new("1920x1080"))),
        Some((1920, 1080))
    );
    assert_eq!(window_pixels(Some(OsStr::new("800X600"))), Some((800, 600)));
    for invalid in ["", "1920", "0x1080", "1920x", "wide x tall", "-1x5"] {
        assert_eq!(window_pixels(Some(OsStr::new(invalid))), None, "{invalid}");
    }
}

#[test]
fn player_renders_without_the_editor_grid_or_gizmos() {
    let options = renderer_options();
    assert!(!options.grid);
    assert!(options.gizmos.is_none());
}

#[test]
fn player_draws_no_object_ids() {
    assert!(!renderer_options().object_ids);
}

#[test]
fn player_renders_one_sample_per_pixel_as_the_game_panel_does() {
    assert_eq!(renderer_options().samples, 1);
}

#[test]
fn player_renders_shadows() {
    assert!(renderer_options().shadows);
}

#[test]
fn player_reads_startup_scene_and_window_from_sandbox_project() {
    let settings = ProjectRuntimeSettings::load(project_directory()).unwrap();

    let startup_scene = settings.startup_scene_path().unwrap();
    assert_eq!(
        startup_scene,
        project_directory().join("assets/showcase/main/showcase.cxscene")
    );
    assert!(startup_scene.is_file(), "{}", startup_scene.display());

    let options = native_options(&settings, player_icon(&settings));
    assert_eq!(
        options.viewport.inner_size,
        Some(egui::vec2(
            settings.window().width as f32,
            settings.window().height as f32
        ))
    );
    assert!(!options.persist_window);
    assert_eq!(
        options.viewport.app_id.as_deref(),
        Some(super::PLAYER_APP_ID)
    );
    assert_eq!(
        settings.window().icon,
        None,
        "the sandbox uses the Calyx icon"
    );
    assert_eq!(options.viewport.icon, Some(crate::runtime::calyx_icon()));
}

#[test]
fn player_reads_the_demo_projects_own_startup_scene() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demos");
    let settings = ProjectRuntimeSettings::load(&root).unwrap();

    assert_eq!(settings.assets_directory(), root.join("assets"));
    let startup_scene = settings.startup_scene_path().unwrap();
    assert_eq!(startup_scene, root.join("assets/rpg_poly/rpg_poly.cxscene"));
    assert!(startup_scene.is_file(), "{}", startup_scene.display());
    assert_eq!(
        native_options(&settings, player_icon(&settings))
            .viewport
            .inner_size,
        Some(settings.window().inner_size())
    );
}
