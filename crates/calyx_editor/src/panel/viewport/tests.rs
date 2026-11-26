use super::{GizmoDrag, PanelViewport};
use crate::scene_document::SceneDocument;
use crate::scene_edit::SceneEdit;
use calyx_engine::context::GameContext;
use calyx_engine::test_support::{test_asset_context_with_assets, test_game_context};
use nalgebra_glm::{translation, vec3, Mat4};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn document() -> SceneDocument {
    SceneDocument::new(Path::new("gizmo-drag-test-project"))
}

/// A scratch asset root a scene can be saved into, removed on drop.
struct AssetRoot(PathBuf);

impl AssetRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("gizmo-drag-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("asset root should be created");
        Self(dunce::canonicalize(&root).expect("asset root should canonicalize"))
    }

    fn scene_file(&self) -> PathBuf {
        self.0.join("scene.cxscene")
    }
}

impl Drop for AssetRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn game_with_object() -> (GameContext, Uuid, Mat4) {
    with_object(test_game_context())
}

/// A game whose assets come from `root`, so its scene can be saved there.
fn game_with_object_in(root: &AssetRoot) -> (GameContext, Uuid, Mat4) {
    with_object(GameContext::new(test_asset_context_with_assets(vec![root
        .0
        .clone()])))
}

fn with_object(mut game: GameContext) -> (GameContext, Uuid, Mat4) {
    let scene = game.scenes.current_scene_mut();
    let object = scene.create(None, None);
    let start = translation(&vec3(1.0, 2.0, 3.0));
    scene.set_transform(object, &start);
    let id = scene.uuid(object);
    (game, id, start)
}

fn local_matrix(game: &GameContext, id: Uuid) -> Mat4 {
    let scene = game.scenes.simulation_scene();
    scene.transform(scene.find(id).unwrap()).matrix()
}

fn drag(game: &mut GameContext, id: Uuid) -> GizmoDrag {
    drag_in(&document(), game, id)
}

fn drag_in(document: &SceneDocument, game: &mut GameContext, id: Uuid) -> GizmoDrag {
    let object = game.scenes.simulation_scene().find(id).unwrap();
    let mut drag = GizmoDrag::begin(document, game, object);
    drag.move_to(game, translation(&vec3(5.0, 0.0, 0.0)));
    drag.move_to(game, translation(&vec3(9.0, 0.0, 0.0)));
    drag
}

#[test]
fn a_finished_gizmo_drag_is_one_undo_step() {
    let (mut game, id, start) = game_with_object();
    let mut document = SceneDocument::new(Path::new("gizmo-drag-test-project"));

    drag(&mut game, id).commit(&mut document, &game);
    assert_eq!(local_matrix(&game, id), translation(&vec3(9.0, 0.0, 0.0)));
    assert!(document.is_dirty());

    document.undo(&mut game);
    assert_eq!(local_matrix(&game, id), start);
}

#[test]
fn an_abandoned_gizmo_drag_restores_the_transform() {
    let root = AssetRoot::new();
    let (mut game, id, start) = game_with_object_in(&root);
    let mut document = document();
    document
        .save(&mut game, root.scene_file())
        .expect("the scene saves");

    drag_in(&document, &mut game, id).abandon(&mut document, &mut game);

    assert_eq!(local_matrix(&game, id), start, "the drag is rolled back");
    assert!(!document.can_undo(&game));
    assert!(!document.is_dirty());
}

#[test]
fn an_abandoned_simulation_drag_restores_the_simulation_transform() {
    let (mut game, id, start) = game_with_object();
    game.scenes.start_simulation();

    drag(&mut game, id).abandon(&mut document(), &mut game);

    assert_eq!(local_matrix(&game, id), start);
}

#[test]
fn an_abandoned_drag_restores_the_authoring_scene_after_simulation_starts() {
    let (mut game, id, start) = game_with_object();
    let drag = drag(&mut game, id);
    game.scenes.start_simulation();

    drag.abandon(&mut document(), &mut game);

    let scene = game.scenes.current_scene();
    assert_eq!(scene.transform(scene.find(id).unwrap()).matrix(), start);
}

#[test]
fn an_abandoned_drag_keeps_a_transform_written_after_it() {
    // Such as an undo made while the viewport was hidden mid-drag.
    let (mut game, id, _) = game_with_object();
    let drag = drag(&mut game, id);
    let elsewhere = translation(&vec3(0.0, 4.0, 0.0));
    let scene = game.scenes.current_scene_mut();
    let object = scene.find(id).unwrap();
    scene.set_transform(object, &elsewhere);

    drag.abandon(&mut document(), &mut game);

    assert_eq!(local_matrix(&game, id), elsewhere);
}

#[test]
fn an_abandoned_drag_keeps_a_transform_saved_during_it() {
    let root = AssetRoot::new();
    let (mut game, id, _) = game_with_object_in(&root);
    let mut document = document();
    let drag = drag_in(&document, &mut game, id);
    // Saved while the viewport was hidden, with the drag's transform.
    document
        .save(&mut game, root.scene_file())
        .expect("the scene saves");

    drag.abandon(&mut document, &mut game);

    assert_eq!(local_matrix(&game, id), translation(&vec3(9.0, 0.0, 0.0)));
    assert!(!document.is_dirty(), "the scene matches the file");
}

#[test]
fn an_abandoned_drag_keeps_a_transform_an_edit_recorded_during_it() {
    // Such as a remote command, which captures the whole scene.
    let (mut game, id, _) = game_with_object();
    let mut document = document();
    let drag = drag_in(&document, &mut game, id);
    let mut edit = SceneEdit::begin("Create object");
    edit.scene_mut(&mut game).create(None, None);
    edit.commit(&mut document, &game);

    drag.abandon(&mut document, &mut game);
    let moved = translation(&vec3(9.0, 0.0, 0.0));
    assert_eq!(local_matrix(&game, id), moved);

    // Undoing and redoing the edit agree with the scene.
    document.undo(&mut game);
    assert_eq!(local_matrix(&game, id), moved);
    document.redo(&mut game);
    assert_eq!(local_matrix(&game, id), moved);
}

#[test]
fn changing_the_selection_mid_drag_abandons_the_drag() {
    let (mut game, a, start) = game_with_object();
    let b = {
        let scene = game.scenes.current_scene_mut();
        let object = scene.create(None, None);
        scene.uuid(object)
    };
    let mut document = document();
    let mut viewport = PanelViewport::default();
    viewport.transform_edit = Some(drag_in(&document, &mut game, a));

    viewport.end_mismatched_drag(&mut document, &mut game, Some(a));
    assert!(
        viewport.transform_edit.is_some(),
        "same object: keeps going"
    );

    viewport.end_mismatched_drag(&mut document, &mut game, Some(b));
    assert!(viewport.transform_edit.is_none());
    assert!(viewport.drag_suspended, "the gizmo waits for the release");
    assert_eq!(local_matrix(&game, a), start);
    assert!(!document.can_undo(&game));
}

#[test]
fn an_abandoned_drag_of_a_removed_object_does_nothing() {
    let (mut game, id, _) = game_with_object();
    let drag = drag(&mut game, id);
    let scene = game.scenes.current_scene_mut();
    let object = scene.find(id).unwrap();
    scene.delete(object);
    scene.flush_deletes();

    drag.abandon(&mut document(), &mut game);

    assert!(game.scenes.current_scene().find(id).is_none());
}

#[test]
fn viewport_pass_continuity_rejects_inactive_tab_gaps() {
    assert!(PanelViewport::viewport_pass_is_continuous(Some(8), 9));
    assert!(PanelViewport::viewport_pass_is_continuous(Some(8), 8));
    assert!(!PanelViewport::viewport_pass_is_continuous(Some(8), 10));
}

#[test]
fn transform_shortcuts_require_viewport_focus() {
    assert!(PanelViewport::transform_shortcuts_enabled(true, false));
    assert!(!PanelViewport::transform_shortcuts_enabled(false, false));
    assert!(!PanelViewport::transform_shortcuts_enabled(true, true));
}
