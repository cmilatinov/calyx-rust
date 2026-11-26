use super::{common_value_path, restore_component_values, set_json_value, SceneDocument};
use crate::inspector::edit_tracker::{self, ComponentKey};
use crate::inspector::type_inspector::InspectorResponse;
use crate::scene_edit::SceneEdit;
use crate::scene_history::InspectorValueChange;
use crate::widgets::{ThumbnailPriority, ThumbnailRequest, ThumbnailService, ThumbnailStatus};
use calyx_engine::component::ComponentTransform;
use calyx_engine::context::GameContext;
use calyx_engine::test_support::{test_asset_context_with_assets, test_game_context};
use calyx_engine::utils::TypeUuid;
use nalgebra_glm::{self as glm, Vec3};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A scratch asset root, removed on drop.
struct AssetRoot(PathBuf);

impl AssetRoot {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("calyx-scene-document-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("asset root should be created");
        Self(dunce::canonicalize(&root).expect("asset root should canonicalize"))
    }

    fn scene_file(&self) -> PathBuf {
        self.0.join("scene.cxscene")
    }
}

impl Drop for AssetRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn document() -> SceneDocument {
    SceneDocument::new(Path::new("scene-document-test-project"))
}

fn object_count(game: &GameContext) -> usize {
    game.scenes.current_scene().objects().count()
}

fn add_object(document: &mut SceneDocument, game: &mut GameContext) {
    let mut edit = SceneEdit::begin("Add object");
    edit.scene_mut(game).create(None, None);
    edit.commit(document, game);
}

/// Saves a scene with `objects` objects into `root`, then opens it in a
/// fresh game whose assets come from `root`.
fn open_saved_scene(root: &AssetRoot, objects: usize) -> (SceneDocument, GameContext) {
    let mut writer = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let mut writer_document = document();
    for _ in 0..objects {
        add_object(&mut writer_document, &mut writer);
    }
    writer_document
        .save(&mut writer, root.scene_file())
        .expect("the scene saves");

    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let mut document = document();
    assert!(document.open_file(&mut game, root.scene_file()));
    (document, game)
}

#[test]
fn open_loads_the_saved_scene_clean() {
    let root = AssetRoot::new();

    let (document, game) = open_saved_scene(&root, 2);

    assert_eq!(object_count(&game), 2);
    assert_eq!(
        game.scenes.current_scene_meta().file.as_deref(),
        Some(root.scene_file().as_path())
    );
    assert!(!document.is_dirty());
    assert!(!document.can_undo(&game));
}

#[test]
fn open_of_an_unregistered_file_keeps_the_current_scene() {
    let mut game = test_game_context();
    let mut document = document();
    add_object(&mut document, &mut game);

    assert!(!document.open_file(&mut game, PathBuf::from("missing.cxscene")));

    assert_eq!(object_count(&game), 1);
    assert!(document.can_undo(&game));
}

#[test]
fn edit_marks_the_document_dirty_and_undo_restores_it_clean() {
    let root = AssetRoot::new();
    let (mut document, mut game) = open_saved_scene(&root, 1);

    add_object(&mut document, &mut game);
    assert_eq!(object_count(&game), 2);
    assert!(document.is_dirty());
    assert!(document.can_undo(&game));

    document.undo(&mut game);
    assert_eq!(object_count(&game), 1);
    assert!(!document.is_dirty(), "the scene matches its file again");
    assert!(document.can_redo(&game));

    document.redo(&mut game);
    assert_eq!(object_count(&game), 2);
    assert!(document.is_dirty());
    assert!(!document.can_redo(&game));
}

#[test]
fn save_writes_the_edited_scene_and_marks_it_clean() {
    let root = AssetRoot::new();
    let (mut document, mut game) = open_saved_scene(&root, 1);
    add_object(&mut document, &mut game);

    document
        .save(&mut game, root.scene_file())
        .expect("the scene saves");

    assert!(!document.is_dirty());
    let mut reopened = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    assert!(
        SceneDocument::new(Path::new("other-project")).open_file(&mut reopened, root.scene_file())
    );
    assert_eq!(object_count(&reopened), 2);

    // Undoing past the save leaves the scene different from its file.
    document.undo(&mut game);
    assert!(document.is_dirty());
}

#[test]
fn save_requests_the_saved_scene_thumbnail_at_high_priority() {
    let root = AssetRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let thumbnails = ThumbnailService::default();
    let mut document = document().with_thumbnails(thumbnails.handle());
    add_object(&mut document, &mut game);

    document
        .save(&mut game, root.scene_file())
        .expect("the scene saves");

    let key = {
        let registry = game.assets.registries.assets.read();
        ThumbnailRequest::from_asset_path(&registry, &root.scene_file())
            .expect("a saved scene has a thumbnail")
            .key()
    };
    assert_eq!(
        thumbnails.status(key),
        ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High
        }
    );
}

#[test]
fn a_scene_saved_to_a_new_path_opens_in_the_same_session() {
    let root = AssetRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let mut document = document();
    add_object(&mut document, &mut game);
    add_object(&mut document, &mut game);
    let file = root.0.join("levels").join("saved-as.cxscene");

    document
        .save(&mut game, file.clone())
        .expect("a scene saves into an asset root");
    document.open_default(&mut game);
    assert_ne!(object_count(&game), 2, "the default scene replaced it");

    // The registry the editor opens through, with no watcher event yet.
    assert!(document.open_file(&mut game, file.clone()));
    assert_eq!(object_count(&game), 2);
    assert_eq!(
        game.scenes.current_scene_meta().file.as_deref(),
        Some(file.as_path())
    );
}

#[test]
fn a_save_outside_every_asset_root_is_refused_and_says_why() {
    let root = AssetRoot::new();
    let outside = AssetRoot::new();
    let mut game = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
    let mut document = document();
    add_object(&mut document, &mut game);
    let file = outside.scene_file();

    let error = document
        .save(&mut game, file.clone())
        .expect_err("a scene outside every asset root could not be opened again");

    assert!(error.contains("outside every asset root"), "{error}");
    assert!(error.contains(&root.0.display().to_string()), "{error}");
    assert!(!file.exists(), "nothing is written");
    assert!(document.is_dirty(), "the scene keeps its unsaved changes");
    assert_eq!(game.scenes.current_scene_meta().file, None);
}

#[test]
fn a_pending_inspector_drag_is_one_undo_step() {
    let mut game = test_game_context();
    let mut document = document();
    let scene = game.scenes.current_scene_mut();
    let object = scene.create(None, None);
    let key = ComponentKey {
        game_object: scene.uuid(object),
        component: ComponentTransform::type_uuid(),
    };
    let set_position = |game: &mut GameContext, x: f32| {
        let scene = game.scenes.current_scene_mut();
        scene.set_transform(object, &glm::translation(&Vec3::new(x, 0.0, 0.0)));
    };
    let serialize = |game: &GameContext| {
        let components = game.assets.registries.components.read();
        edit_tracker::serialize_component(game.scenes.current_scene(), &components, key)
    };
    document.select_inspector_object(&game, Some(key.game_object));
    assert!(document.track_inspector_edits(&game));
    let baseline = serialize(&game);
    document.inspector_edits_mut().set_baseline(key, baseline);

    // A drag over two frames that has not stopped yet.
    for (x, drag_started) in [(1.0, true), (2.0, false)] {
        set_position(&mut game, x);
        let response = InspectorResponse {
            changed: true,
            drag_started,
            drag_stopped: false,
        };
        assert!(!document.inspector_edits_mut().report(key, response, true));
        document.inspector_shown(&game, true, false, None);
    }
    assert!(document.is_dirty());
    assert!(document.can_undo(&game), "the pending drag can be undone");
    assert!(!document.can_redo(&game), "pending edits block redo");

    document.undo(&mut game);
    let position = game.scenes.current_scene().world_transform(object).position;
    assert!(position.norm() < 1e-5, "position {position:?}");
    assert!(!document.can_undo(&game), "the drag was one step");

    document.redo(&mut game);
    let position = game.scenes.current_scene().world_transform(object).position;
    assert!((position - Vec3::new(2.0, 0.0, 0.0)).norm() < 1e-5);
    assert!(!document.can_redo(&game));
}

#[test]
fn simulation_blocks_edit_undo_and_redo() {
    let mut game = test_game_context();
    let mut document = document();
    add_object(&mut document, &mut game);
    game.scenes.start_simulation();

    assert!(SceneDocument::begin_edit(&game).is_none());
    assert!(!document.track_inspector_edits(&game));
    assert!(!document.can_undo(&game));
    document.undo(&mut game);
    assert_eq!(object_count(&game), 1, "undo leaves the scene alone");
    assert!(
        game.scenes.has_simulation_scene(),
        "undo keeps the simulation"
    );

    game.scenes.stop_simulation();
    assert!(document.can_undo(&game));
    document.undo(&mut game);
    assert_eq!(object_count(&game), 0);
}

#[test]
fn restoring_a_parent_position_moves_the_child() {
    let registries = calyx_engine::test_support::test_registries();
    let mut scene = registries.scene();
    let parent = scene.create(None, None);
    let child = scene.create(None, Some(parent));
    scene.set_transform(parent, &glm::translation(&Vec3::new(1.0, 0.0, 0.0)));
    scene.set_transform(child, &glm::translation(&Vec3::new(0.0, 1.0, 0.0)));
    assert!((scene.world_transform(child).position - Vec3::new(1.0, 1.0, 0.0)).norm() < 1e-5);

    let component_registry = registries.components.read();
    let component = component_registry
        .component(ComponentTransform::type_uuid())
        .expect("transform should be registered");
    restore_component_values(
        &mut scene,
        parent,
        component,
        &[(
            vec!["transform".into(), "position".into()],
            json!([5.0, 0.0, 0.0]),
        )],
    )
    .expect("the position should restore");

    let child_world = scene.world_transform(child).position;
    assert!(
        (child_world - Vec3::new(5.0, 1.0, 0.0)).norm() < 1e-5,
        "child world position {child_world:?}"
    );
}

#[test]
fn edit_labels_name_the_field_every_change_shares() {
    let change = |path: &[&str]| InspectorValueChange {
        value_path: path.iter().map(|segment| segment.to_string()).collect(),
        before: json!(0.0),
        after: json!(1.0),
    };
    assert_eq!(
        common_value_path(&[
            change(&["transform", "rotation", "0"]),
            change(&["transform", "rotation", "3"]),
        ]),
        vec!["transform", "rotation"]
    );
    assert_eq!(common_value_path(&[change(&["enabled"])]), vec!["enabled"]);
    assert!(common_value_path(&[change(&["mass"]), change(&["enabled"])]).is_empty());
}

#[test]
fn json_value_patch_preserves_other_component_values() {
    let mut component = json!({
        "position": [1.0, 2.0, 3.0],
        "enabled": true
    });

    assert!(set_json_value(
        &mut component,
        &["position".into(), "1".into()],
        json!(9.0)
    ));

    assert_eq!(component["position"], json!([1.0, 9.0, 3.0]));
    assert_eq!(component["enabled"], json!(true));
}
