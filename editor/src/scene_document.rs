//! The editor's scene document: the authoring scene's undo history, inspector
//! value debounce, recovery autosave, dirty state and file identity, behind
//! edit, undo, redo, open and save.
//!
//! Edits are refused while a simulation copy of the scene exists. That rule
//! lives in [`authoring`], which every edit, undo and redo goes through.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui;
use engine::component::Component;
use engine::context::GameContext;
use engine::scene::{GameObject, Scene};
use serde_json::Value;
use uuid::Uuid;

use crate::scene_autosave::EditorSceneAutosave;
use crate::scene_file::{scene_fingerprint, write_scene_file};
use crate::scene_history::{
    InspectorValueDebouncer, InspectorValueEdit, InspectorValueKey, SceneEditSnapshot,
    SceneHistory, SceneHistoryRestore,
};

/// One changed component value: its path in the component's JSON, then the
/// value before and after the change.
pub type ValueChange = (Vec<String>, Value, Value);

pub struct SceneDocument {
    history: SceneHistory,
    pending_values: InspectorValueDebouncer,
    autosave: EditorSceneAutosave,
}

/// The simulation guard: `game` while its authoring scene may be edited, that
/// is while no simulation copy of it exists.
fn authoring<G: Deref<Target = GameContext>>(game: G) -> Option<G> {
    (!game.scenes.has_simulation_scene()).then_some(game)
}

#[derive(Clone, Copy)]
enum HistoryStep {
    Undo,
    Redo,
}

impl SceneDocument {
    pub fn new(project_path: &Path) -> Self {
        Self {
            history: SceneHistory::default(),
            pending_values: InspectorValueDebouncer::default(),
            autosave: EditorSceneAutosave::new(project_path),
        }
    }

    /// Whether the authoring scene differs from its saved file.
    pub fn is_dirty(&self) -> bool {
        self.autosave.is_dirty()
    }

    /// Captures the authoring scene before an edit, for [`Self::commit_edit`].
    /// `None` while the scene cannot be edited.
    pub fn begin_edit(game: &GameContext) -> Option<SceneEditSnapshot> {
        authoring(game).map(|game| SceneEditSnapshot::capture(game.scenes.current_scene()))
    }

    /// Records the edit made since `before` as one undo step. Without a
    /// `before` snapshot the scene is only marked dirty.
    pub fn commit_edit(
        &mut self,
        game: &GameContext,
        label: impl Into<String>,
        before: Option<SceneEditSnapshot>,
    ) {
        let Some(game) = authoring(game) else {
            return;
        };
        self.commit_pending_values(game);

        let Some(before) = before else {
            self.autosave.mark_dirty();
            return;
        };

        let after = SceneEditSnapshot::capture(game.scenes.current_scene());
        let fingerprint = after.fingerprint().map(str::to_owned);
        if self.history.push(label, before, after) {
            self.autosave.sync_dirty_to_fingerprint(fingerprint);
        }
    }

    /// Records value edits the inspector already applied to `component` on
    /// `game_object`. Edits to the same value merge until they settle; a
    /// change at a `structural` path, or one that resizes a list, is its own
    /// undo step at once.
    pub fn record_values(
        &mut self,
        game: &GameContext,
        game_object: Uuid,
        component: Uuid,
        component_name: &str,
        changes: Vec<ValueChange>,
        structural: &[Vec<String>],
    ) {
        let Some(game) = authoring(game) else {
            return;
        };
        if changes.is_empty() {
            return;
        }

        for (value_path, before, after) in changes {
            let value_name = if value_path.is_empty() {
                "value".to_owned()
            } else {
                value_path.join(".")
            };
            let edit = InspectorValueEdit {
                label: format!("Edit {component_name} {value_name}"),
                key: InspectorValueKey {
                    game_object,
                    component,
                    value_path,
                },
                before,
                after,
            };
            if structural.contains(&edit.key.value_path) || edit.resizes_list() {
                // Adding, removing or moving an element is one undo step. Pending
                // element edits commit first so their indices still match on undo.
                self.commit_pending_values(game);
                self.history.push_inspector_value(edit);
            } else {
                self.pending_values.record(edit, Instant::now());
            }
        }
        self.sync_dirty(game);
    }

    /// Commits every pending value edit to the history now.
    pub fn flush_pending_values(&mut self, game: &GameContext) {
        match authoring(game) {
            Some(game) => self.commit_pending_values(game),
            None => self.pending_values.clear(),
        }
    }

    pub fn can_undo(&self, game: &GameContext) -> bool {
        authoring(game).is_some() && (self.history.can_undo() || self.pending_values.has_pending())
    }

    pub fn can_redo(&self, game: &GameContext) -> bool {
        authoring(game).is_some() && !self.pending_values.has_pending() && self.history.can_redo()
    }

    pub fn undo(&mut self, game: &mut GameContext) {
        self.step(game, HistoryStep::Undo);
    }

    pub fn redo(&mut self, game: &mut GameContext) {
        self.step(game, HistoryStep::Redo);
    }

    /// Opens the scene file at `file`, which must be a registered asset. Keeps
    /// the current scene when its recovery autosave or the load fails.
    pub fn open_file(&mut self, game: &mut GameContext, file: PathBuf) -> bool {
        if !self.autosave.prepare_scene_change(game) {
            return false;
        }
        let scene = match game
            .assets
            .registries
            .assets
            .read()
            .reload_by_path(file.as_path())
        {
            Ok(scene) => scene,
            Err(error) => {
                log::error!("Failed to open scene {}: {}", file.display(), error);
                return false;
            }
        };
        game.scenes.load_scene(scene.readonly());
        if game.scenes.current_scene_meta().file.is_none() {
            game.scenes.set_current_scene_file(Some(file.clone()));
        }
        self.autosave.scene_loaded(game);
        self.clear_history();
        let object_count = game.scenes.current_scene().objects().count();
        log::info!("Opened scene {} ({} objects)", file.display(), object_count);
        true
    }

    /// Opens the scene asset `asset_id`. Keeps the current scene when its
    /// recovery autosave or the load fails.
    pub fn open_asset(&mut self, game: &mut GameContext, asset_id: Uuid) -> bool {
        if !self.autosave.prepare_scene_change(game) {
            return false;
        }
        let (scene, label) = {
            let registry = game.assets.registries.assets.read();
            let label = registry
                .asset_meta_from_id(asset_id)
                .and_then(|meta| meta.path.map(|path| path.display().to_string()))
                .unwrap_or_else(|| asset_id.to_string());
            (registry.reload_by_id::<Scene>(asset_id), label)
        };

        match scene {
            Ok(scene) => {
                game.scenes.load_scene(scene.readonly());
                self.autosave.scene_loaded(game);
                self.clear_history();
                let object_count = game.scenes.current_scene().objects().count();
                log::info!("Opened scene {label} ({object_count} objects)");
                true
            }
            Err(error) => {
                log::error!("Failed to open scene {label}: {error}");
                false
            }
        }
    }

    /// Replaces the authoring scene with a copy of the default scene, with no
    /// file.
    pub fn open_default(&mut self, game: &mut GameContext) {
        if !self.autosave.prepare_scene_change(game) {
            return;
        }
        game.scenes.load_default_scene();
        self.autosave.new_scene_loaded(game.scenes.current_scene());
        self.clear_history();
        log::info!("Created new scene from default scene");
    }

    /// Writes the authoring scene to `file`, which becomes its file.
    pub fn save(&mut self, game: &mut GameContext, file: PathBuf) -> bool {
        let scene = game.scenes.current_scene();
        let object_count = scene.objects().count();
        if let Err(error) = write_scene_file(&file, scene) {
            log::error!("Failed to save scene {}: {error}", file.display());
            return false;
        }
        log::info!("Saved scene {} ({object_count} objects)", file.display());

        let previous_file = game.scenes.current_scene_meta().file.clone();
        game.scenes.set_current_scene_file(Some(file.clone()));
        self.autosave
            .scene_saved(previous_file.as_deref(), &file, game.scenes.current_scene());
        true
    }

    /// Runs once per frame: commits value edits that have settled and starts
    /// or finishes a background recovery autosave when one is due.
    pub fn update(&mut self, game: &GameContext) {
        match authoring(game) {
            Some(game) => {
                let edits = self.pending_values.drain_due(Instant::now());
                self.commit_values(game, edits);
            }
            None => self.pending_values.clear(),
        }
        self.autosave.update(game);
    }

    /// Shows the prompt offering a newer recovery autosave of the open scene.
    pub fn show_recovery_prompt(&mut self, ctx: &egui::Context, game: &mut GameContext) {
        if self.autosave.show_recovery_prompt(ctx, game) {
            self.clear_history();
        }
    }

    /// Commits pending edits and writes a final recovery autosave.
    pub fn shutdown(&mut self, game: &GameContext) {
        self.flush_pending_values(game);
        self.autosave.shutdown(game);
    }

    fn step(&mut self, game: &mut GameContext, step: HistoryStep) {
        let Some(game) = authoring(game) else {
            return;
        };
        self.commit_pending_values(game);
        let (restore, verb) = match step {
            HistoryStep::Undo => (self.history.undo(), "Undid"),
            HistoryStep::Redo => (self.history.redo(), "Redid"),
        };
        match restore {
            None => {}
            Some(SceneHistoryRestore::Scene {
                label,
                snapshot,
                fingerprint,
            }) => {
                game.scenes.restore_current_scene_snapshot(snapshot);
                self.autosave.sync_dirty_to_fingerprint(fingerprint);
                log::info!("{verb} scene edit: {label}");
            }
            Some(SceneHistoryRestore::InspectorValue { label, key, value }) => {
                if restore_inspector_value(game, &key, value) {
                    self.sync_dirty(game);
                    log::info!("{verb} scene edit: {label}");
                }
            }
        }
    }

    fn commit_pending_values(&mut self, game: &GameContext) {
        let edits = self.pending_values.drain_all();
        self.commit_values(game, edits);
    }

    fn commit_values(&mut self, game: &GameContext, edits: Vec<InspectorValueEdit>) {
        let mut committed = false;
        for edit in edits {
            committed |= self.history.push_inspector_value(edit);
        }
        if committed {
            self.sync_dirty(game);
        }
    }

    fn sync_dirty(&mut self, game: &GameContext) {
        self.autosave
            .sync_dirty_to_fingerprint(scene_fingerprint(game.scenes.current_scene()));
    }

    fn clear_history(&mut self) {
        self.history.clear();
        self.pending_values.clear();
    }
}

fn restore_inspector_value(game: &mut GameContext, key: &InspectorValueKey, value: Value) -> bool {
    let component_registry_ref = game.assets.registries.components.clone();
    let component_registry = component_registry_ref.read();
    let Some(component) = component_registry.component(key.component) else {
        log::warn!(
            "Skipped inspector history restore because component {} is no longer registered",
            key.component
        );
        return false;
    };
    let scene = game.scenes.current_scene_mut();
    let Some(game_object) = scene.find(key.game_object) else {
        log::warn!(
            "Skipped inspector history restore because game object {} no longer exists",
            key.game_object
        );
        return false;
    };
    match restore_component_value(scene, game_object, component, &key.value_path, value) {
        Ok(()) => true,
        Err(err) => {
            log::warn!(
                "Skipped inspector history restore of component {} on game object {} at {:?}: {err}",
                key.component,
                key.game_object,
                key.value_path
            );
            false
        }
    }
}

/// Why a recorded inspector value could not be written back to its component.
#[derive(Debug, PartialEq, Eq)]
enum ValueRestoreError {
    MissingComponent,
    NotSerializable,
    MissingValuePath,
    NotDeserializable,
}

impl std::fmt::Display for ValueRestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingComponent => "the component is missing from the game object",
            Self::NotSerializable => "the component cannot be serialized",
            Self::MissingValuePath => "the value path no longer exists",
            Self::NotDeserializable => "the component cannot be deserialized",
        })
    }
}

/// Writes `value` at `value_path` inside `game_object`'s `component`, leaving
/// the component's other values as they are.
fn restore_component_value(
    scene: &mut Scene,
    game_object: GameObject,
    component: &dyn Component,
    value_path: &[String],
    value: Value,
) -> Result<(), ValueRestoreError> {
    scene
        .write_component_dyn(game_object, component, |instance| {
            let mut serialized = instance
                .serialize()
                .ok_or(ValueRestoreError::NotSerializable)?;
            if !set_json_value(&mut serialized, value_path, value) {
                return Err(ValueRestoreError::MissingValuePath);
            }
            if !instance.deserialize_in_place(&serialized) {
                return Err(ValueRestoreError::NotDeserializable);
            }
            Ok(())
        })
        .unwrap_or(Err(ValueRestoreError::MissingComponent))
}

pub(crate) fn set_json_value(target: &mut Value, path: &[String], value: Value) -> bool {
    let Some((segment, remaining)) = path.split_first() else {
        *target = value;
        return true;
    };

    match target {
        Value::Object(values) => values
            .get_mut(segment)
            .is_some_and(|target| set_json_value(target, remaining, value)),
        Value::Array(values) => segment
            .parse::<usize>()
            .ok()
            .and_then(|index| values.get_mut(index))
            .is_some_and(|target| set_json_value(target, remaining, value)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{restore_component_value, set_json_value, SceneDocument};
    use engine::component::ComponentTransform;
    use engine::context::GameContext;
    use engine::test_support::{test_asset_context_with_assets, test_game_context};
    use engine::utils::TypeUuid;
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
        let before = SceneDocument::begin_edit(game);
        game.scenes.current_scene_mut().create(None, None);
        document.commit_edit(game, "Add object", before);
    }

    /// Saves a scene with `objects` objects into `root`, then opens it in a
    /// fresh game whose assets come from `root`.
    fn open_saved_scene(root: &AssetRoot, objects: usize) -> (SceneDocument, GameContext) {
        let mut writer = test_game_context();
        let mut writer_document = document();
        for _ in 0..objects {
            add_object(&mut writer_document, &mut writer);
        }
        assert!(writer_document.save(&mut writer, root.scene_file()));

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

        assert!(document.save(&mut game, root.scene_file()));

        assert!(!document.is_dirty());
        let mut reopened = GameContext::new(test_asset_context_with_assets(vec![root.0.clone()]));
        assert!(SceneDocument::new(Path::new("other-project"))
            .open_file(&mut reopened, root.scene_file()));
        assert_eq!(object_count(&reopened), 2);

        // Undoing past the save leaves the scene different from its file.
        document.undo(&mut game);
        assert!(document.is_dirty());
    }

    #[test]
    fn value_edits_merge_into_one_undo_step() {
        let mut game = test_game_context();
        let mut document = document();
        let scene = game.scenes.current_scene_mut();
        let object = scene.create(None, None);
        let object_id = scene.uuid(object);
        let transform = ComponentTransform::type_uuid();
        let path = vec!["transform".to_owned(), "position".to_owned()];
        let set_position = |game: &mut GameContext, x: f32| {
            let scene = game.scenes.current_scene_mut();
            scene.set_transform(object, &glm::translation(&Vec3::new(x, 0.0, 0.0)));
        };

        for (before, after) in [(0.0, 1.0), (1.0, 2.0)] {
            set_position(&mut game, after);
            document.record_values(
                &game,
                object_id,
                transform,
                "Transform",
                vec![(
                    path.clone(),
                    json!([before, 0.0, 0.0]),
                    json!([after, 0.0, 0.0]),
                )],
                &[],
            );
        }
        assert!(document.can_undo(&game));
        assert!(!document.can_redo(&game), "pending edits block redo");

        document.undo(&mut game);
        let position = game.scenes.current_scene().world_transform(object).position;
        assert!(position.norm() < 1e-5, "position {position:?}");

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
        let registries = engine::test_support::test_registries();
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
        restore_component_value(
            &mut scene,
            parent,
            component,
            &["transform".into(), "position".into()],
            json!([5.0, 0.0, 0.0]),
        )
        .expect("the position should restore");

        let child_world = scene.world_transform(child).position;
        assert!(
            (child_world - Vec3::new(5.0, 1.0, 0.0)).norm() < 1e-5,
            "child world position {child_world:?}"
        );
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
}
