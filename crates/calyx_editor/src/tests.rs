use super::{game_aspect_for_start, press_ends_inspector_edits, ActiveSceneState, EditorApp};
use std::path::PathBuf;

#[test]
fn active_scene_title_uses_untitled_fallback() {
    let mut state = ActiveSceneState::default();

    state.set_scene(None, None);
    assert_eq!(
        state.title("Sandbox", None, false),
        "Calyx - Sandbox - Untitled Scene"
    );
}

#[test]
fn active_scene_title_uses_asset_name() {
    let mut state = ActiveSceneState::default();
    let file = PathBuf::from("assets/scene.cxscene");

    state.set_scene(Some("scene".into()), Some(file));
    assert_eq!(
        state.title("Sandbox", None, false),
        "Calyx - Sandbox - scene"
    );
}

#[test]
fn active_scene_title_uses_file_name_fallback() {
    let mut state = ActiveSceneState::default();
    let file = PathBuf::from("assets/scene.cxscene");

    state.set_scene(None, Some(file));
    assert_eq!(
        state.title("Sandbox", None, false),
        "Calyx - Sandbox - scene.cxscene"
    );
}

#[test]
fn active_scene_title_marks_dirty_scene() {
    let mut state = ActiveSceneState::default();

    state.set_scene(Some("scene".into()), None);
    assert_eq!(
        state.title("Sandbox", None, true),
        "Calyx - Sandbox - scene *"
    );
}

#[test]
fn only_presses_outside_the_inspector_end_its_edits() {
    let inspector = egui::Rect::from_min_max(egui::pos2(1000.0, 0.0), egui::pos2(1300.0, 800.0));
    assert!(!press_ends_inspector_edits(
        Some(egui::pos2(1100.0, 200.0)),
        Some(inspector)
    ));
    assert!(press_ends_inspector_edits(
        Some(egui::pos2(100.0, 200.0)),
        Some(inspector)
    ));
    assert!(press_ends_inspector_edits(
        Some(egui::pos2(1100.0, 200.0)),
        None
    ));
    assert!(press_ends_inspector_edits(None, Some(inspector)));
}

#[test]
fn zero_sized_game_viewport_is_not_renderable() {
    assert!(!EditorApp::has_renderable_size(0, 720));
    assert!(!EditorApp::has_renderable_size(1280, 0));
    assert!(EditorApp::has_renderable_size(1280, 720));
}

#[test]
fn resuming_and_stepping_a_play_copy_keep_the_selected_game_aspect() {
    let mut game = calyx_engine::test_support::test_game_context();
    let project_size = Some((1920, 1080));
    assert_eq!(
        game_aspect_for_start(&game.scenes, None, project_size),
        project_size
    );
    game.scenes.start_simulation();

    for selected in [None, Some((4, 3)), Some((21, 9))] {
        game.scenes.pause_simulation();
        assert!(!game.scenes.is_simulating());
        assert_eq!(
            game_aspect_for_start(&game.scenes, selected, project_size),
            selected
        );
        game.scenes.start_simulation();
        assert!(game.scenes.is_simulating());
        assert_eq!(
            game_aspect_for_start(&game.scenes, selected, project_size),
            selected
        );
    }

    game.scenes.stop_simulation();
    assert_eq!(
        game_aspect_for_start(&game.scenes, Some((4, 3)), project_size),
        project_size
    );
    assert_eq!(
        game_aspect_for_start(&game.scenes, Some((4, 3)), None),
        Some((4, 3))
    );
}
