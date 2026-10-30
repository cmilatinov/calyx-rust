//! The editor's scene document: the authoring scene's undo history, the
//! inspector's undo baselines, recovery autosave, dirty state and file
//! identity, behind edit, undo, redo, open and save.
//!
//! Edits are refused while a simulation copy of the scene exists. That rule
//! lives in [`authoring`], which every edit, undo and redo goes through.

use std::ops::Deref;
use std::path::{Path, PathBuf};

use calyx_engine::component::Component;
use calyx_engine::context::GameContext;
use calyx_engine::scene::{GameObject, Scene};
use eframe::egui;
use serde_json::Value;
use uuid::Uuid;

use crate::inspector::component_inspector;
use crate::inspector::edit_tracker::{self, CommittedEdit, InspectorEditTracker};
use crate::scene_autosave::EditorSceneAutosave;
use crate::scene_file::{scene_fingerprint, write_scene_file};
use crate::scene_history::{
    InspectorValueChange, InspectorValueEdit, SceneEditSnapshot, SceneHistory, SceneHistoryRestore,
};

pub struct SceneDocument {
    history: SceneHistory,
    /// The inspector's undo baselines for the selected object's components.
    inspector_edits: InspectorEditTracker,
    autosave: EditorSceneAutosave,
    /// Counts the changes to the undo history and the saved state.
    revision: u64,
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
            inspector_edits: InspectorEditTracker::default(),
            autosave: EditorSceneAutosave::new(project_path),
            revision: 0,
        }
    }

    /// Whether the authoring scene differs from its saved file.
    pub fn is_dirty(&self) -> bool {
        self.autosave.is_dirty()
    }

    /// Captures the authoring scene before an edit, for [`Self::commit_edit`].
    /// `None` while the scene cannot be edited. Panels edit through
    /// [`crate::scene_edit::SceneEdit`], which calls this.
    pub(crate) fn begin_edit(game: &GameContext) -> Option<SceneEditSnapshot> {
        authoring(game).map(|game| SceneEditSnapshot::capture(game.scenes.current_scene()))
    }

    /// Records the edit made since `before` as one undo step.
    pub(crate) fn commit_edit(
        &mut self,
        game: &GameContext,
        label: impl Into<String>,
        before: SceneEditSnapshot,
    ) {
        let Some(game) = authoring(game) else {
            return;
        };
        // The edit wrote the scene outside the inspector, so its baselines
        // are taken again.
        self.refresh_baselines(game);

        let after = SceneEditSnapshot::capture(game.scenes.current_scene());
        let fingerprint = after.fingerprint().map(str::to_owned);
        if self.history.push(label, before, after) {
            self.revision += 1;
            self.autosave.sync_dirty_to_fingerprint(fingerprint);
        }
    }

    /// Changes whenever the undo history or the saved state changes: an edit
    /// recorded, an undo or redo, a save, or another scene opened. A write
    /// made outside the history may be rolled back only while it is
    /// unchanged, since every snapshot taken since holds the write.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Takes a write the authoring scene got outside any edit, such as an
    /// abandoned gizmo drag rolled back: the inspector's baselines are taken
    /// again and the dirty state follows the scene.
    pub(crate) fn scene_written_outside_history(&mut self, game: &GameContext) {
        let Some(game) = authoring(game) else {
            return;
        };
        self.refresh_baselines(game);
        self.sync_dirty(game);
    }

    /// The tracker the inspector shows components through.
    pub fn inspector_edits_mut(&mut self) -> &mut InspectorEditTracker {
        &mut self.inspector_edits
    }

    /// Whether the inspector records its edits this frame. While the scene
    /// cannot be edited it records none and keeps no baselines.
    pub fn track_inspector_edits(&mut self, game: &GameContext) -> bool {
        let track = authoring(game).is_some();
        if !track {
            self.inspector_edits.clear();
        }
        track
    }

    /// Makes `object` the object the inspector tracks, committing the edits
    /// pending on the previous one first.
    pub fn select_inspector_object(&mut self, game: &GameContext, object: Option<Uuid>) {
        if self.inspector_edits.object() != object {
            self.flush_pending_values(game);
            self.inspector_edits.reset(object);
        }
    }

    /// Takes what one frame of the inspector did to a component: whether it
    /// `changed` the scene, whether an edit `completed`, and the undo step it
    /// `committed`, if any.
    pub fn inspector_shown(
        &mut self,
        game: &GameContext,
        changed: bool,
        completed: bool,
        committed: Option<CommittedEdit>,
    ) {
        let Some(game) = authoring(game) else {
            return;
        };
        // The undo step may wait for the drag or text session to end, but
        // the scene has changed now: autosave and scene switches must see it
        // as dirty.
        if changed {
            self.autosave.mark_dirty();
        }
        match committed {
            Some(committed) => self.record_inspector_edit(game, committed),
            // An edit that left the component as it was: undo the dirty mark
            // it set.
            None if completed => self.sync_dirty(game),
            None => {}
        }
    }

    /// Commits every inspector edit still pending, such as a drag in
    /// progress, so it is recorded before another edit or an undo.
    pub fn flush_pending_values(&mut self, game: &GameContext) {
        match authoring(game) {
            Some(game) => self.commit_pending_values(game),
            None => self.inspector_edits.clear(),
        }
    }

    /// Commits pending inspector edits and drops the inspector's baselines,
    /// so each is taken again from the scene, after a write the inspector
    /// did not make.
    pub fn refresh_baselines(&mut self, game: &GameContext) {
        self.flush_pending_values(game);
        self.inspector_edits.clear();
    }

    pub fn can_undo(&self, game: &GameContext) -> bool {
        authoring(game).is_some() && (self.history.can_undo() || self.inspector_edits.has_pending())
    }

    pub fn can_redo(&self, game: &GameContext) -> bool {
        authoring(game).is_some() && !self.inspector_edits.has_pending() && self.history.can_redo()
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
        self.flush_pending_values(game);
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
        self.flush_pending_values(game);
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
        self.flush_pending_values(game);
        if !self.autosave.prepare_scene_change(game) {
            return;
        }
        game.scenes.load_default_scene();
        self.autosave.new_scene_loaded(game.scenes.current_scene());
        self.clear_history();
        log::info!("Created new scene from default scene");
    }

    /// Writes the authoring scene to `file`, which becomes its file, and
    /// registers it as an asset, so it opens at once. A `file` outside every
    /// asset root is refused, since it could never be opened again: the
    /// error says why.
    pub fn save(&mut self, game: &mut GameContext, file: PathBuf) -> Result<(), String> {
        let result = self.write(game, &file);
        if let Err(error) = &result {
            log::error!("Failed to save scene {}: {error}", file.display());
        }
        result
    }

    fn write(&mut self, game: &mut GameContext, file: &Path) -> Result<(), String> {
        {
            let registry = game.assets.registries.assets.read();
            if !registry.is_in_asset_root(file) {
                let roots = registry
                    .asset_paths()
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "{} is outside every asset root ({roots}); the editor opens scenes                      only from an asset root, so save it inside one",
                    file.display()
                ));
            }
        }
        let scene = game.scenes.current_scene();
        let object_count = scene.objects().count();
        write_scene_file(file, scene)?;
        log::info!("Saved scene {} ({object_count} objects)", file.display());
        // The watcher registers a new file too, but later and on its own
        // thread, so a scene opened right after its save could miss it.
        if let Err(error) = game.assets.registries.assets.read().register_file(file) {
            log::warn!(
                "Saved scene {} is not registered as an asset until the editor restarts: {error}",
                file.display()
            );
        }

        let previous_file = game.scenes.current_scene_meta().file.clone();
        game.scenes.set_current_scene_file(Some(file.to_path_buf()));
        self.autosave
            .scene_saved(previous_file.as_deref(), file, game.scenes.current_scene());
        self.revision += 1;
        Ok(())
    }

    /// Commits pending inspector edits and writes the scene's recovery
    /// autosave now, before the editor sets the scene aside. `false` when the
    /// autosave failed.
    pub(crate) fn set_aside(&mut self, game: &GameContext) -> bool {
        self.flush_pending_values(game);
        self.inspector_edits.clear();
        self.autosave.prepare_scene_change(game)
    }

    /// Takes the scene the editor put back in the authoring slot after
    /// [`Self::set_aside`]: its history, dirty state and file are as they
    /// were, but every object was rebuilt.
    pub(crate) fn resumed(&mut self) {
        self.revision += 1;
        self.inspector_edits.reset(None);
    }

    /// Takes the authoring scene as opened from a file the document does not
    /// own, such as a prefab: clean, with no history and no recovery
    /// autosave.
    pub(crate) fn opened_detached(&mut self, game: &GameContext) {
        self.autosave.new_scene_loaded(game.scenes.current_scene());
        self.clear_history();
    }

    /// Marks the authoring scene clean after the caller wrote it to a file
    /// the document does not own. The undo history stays.
    pub(crate) fn saved_detached(&mut self, game: &GameContext) {
        self.autosave.new_scene_loaded(game.scenes.current_scene());
        self.revision += 1;
    }

    /// Runs once per frame: starts or finishes a background recovery
    /// autosave when one is due.
    pub fn update(&mut self, game: &GameContext) {
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
        if restore.is_some() {
            self.revision += 1;
        }
        match restore {
            None => return,
            Some(SceneHistoryRestore::Scene {
                label,
                snapshot,
                fingerprint,
            }) => {
                game.scenes.restore_current_scene_snapshot(snapshot);
                self.autosave.sync_dirty_to_fingerprint(fingerprint);
                log::info!("{verb} scene edit: {label}");
            }
            Some(SceneHistoryRestore::InspectorValue {
                label,
                game_object,
                component,
                values,
            }) => {
                if restore_inspector_values(game, game_object, component, &values) {
                    self.sync_dirty(game);
                    log::info!("{verb} scene edit: {label}");
                }
            }
        }
        self.inspector_edits.clear();
    }

    /// Records an edit the inspector committed as one undo step. Callers
    /// have passed the simulation guard.
    fn record_inspector_edit(&mut self, game: &GameContext, edit: CommittedEdit) {
        let component_name = component_inspector::type_display_name(
            &game.assets.registries.types.read(),
            edit.key.component,
        )
        .unwrap_or_else(|| edit.key.component.to_string());
        let label = match common_value_path(&edit.changes) {
            path if path.is_empty() => format!("Edit {component_name}"),
            path => format!("Edit {component_name} {}", path.join(".")),
        };
        if self.history.push_inspector_value(InspectorValueEdit {
            label,
            game_object: edit.key.game_object,
            component: edit.key.component,
            changes: edit.changes,
        }) {
            self.revision += 1;
            self.sync_dirty(game);
        }
    }

    /// Commits every pending inspector edit. Callers have passed the
    /// simulation guard.
    fn commit_pending_values(&mut self, game: &GameContext) {
        let pending = self.inspector_edits.pending();
        if pending.is_empty() {
            return;
        }
        // Reconcile the dirty mark the pending edits set even when they turn
        // out to change nothing.
        self.sync_dirty(game);
        let edits = {
            let components = game.assets.registries.components.read();
            let scene = game.scenes.current_scene();
            pending
                .into_iter()
                .filter_map(|key| {
                    let current = edit_tracker::serialize_component(scene, &components, key);
                    self.inspector_edits.commit(key, current)
                })
                .collect::<Vec<_>>()
        };
        for edit in edits {
            self.record_inspector_edit(game, edit);
        }
    }

    fn sync_dirty(&mut self, game: &GameContext) {
        self.autosave
            .sync_dirty_to_fingerprint(scene_fingerprint(game.scenes.current_scene()));
    }

    fn clear_history(&mut self) {
        self.revision += 1;
        self.history.clear();
        self.inspector_edits.reset(None);
    }
}

fn restore_inspector_values(
    game: &mut GameContext,
    game_object_id: Uuid,
    component_id: Uuid,
    values: &[(Vec<String>, Value)],
) -> bool {
    let component_registry_ref = game.assets.registries.components.clone();
    let component_registry = component_registry_ref.read();
    let Some(component) = component_registry.component(component_id) else {
        log::warn!(
            "Skipped inspector history restore because component {component_id} is no longer registered"
        );
        return false;
    };
    let scene = game.scenes.current_scene_mut();
    let Some(game_object) = scene.find(game_object_id) else {
        log::warn!(
            "Skipped inspector history restore because game object {game_object_id} no longer exists"
        );
        return false;
    };
    match restore_component_values(scene, game_object, component, values) {
        Ok(()) => true,
        Err(err) => {
            log::warn!(
                "Skipped inspector history restore of component {component_id} on game object {game_object_id}: {err}"
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

/// Writes each value at its path inside `game_object`'s `component`, leaving
/// the component's other values as they are.
fn restore_component_values(
    scene: &mut Scene,
    game_object: GameObject,
    component: &dyn Component,
    values: &[(Vec<String>, Value)],
) -> Result<(), ValueRestoreError> {
    scene
        .write_component_dyn(game_object, component, |instance| {
            let mut serialized = instance
                .serialize()
                .ok_or(ValueRestoreError::NotSerializable)?;
            for (value_path, value) in values {
                if !set_json_value(&mut serialized, value_path, value.clone()) {
                    return Err(ValueRestoreError::MissingValuePath);
                }
            }
            if !instance.deserialize_in_place(&serialized) {
                return Err(ValueRestoreError::NotDeserializable);
            }
            Ok(())
        })
        .unwrap_or(Err(ValueRestoreError::MissingComponent))
}

/// The path prefix every change shares, which names an edit that changed
/// several values of one field, such as a rotation.
fn common_value_path(changes: &[InspectorValueChange]) -> Vec<String> {
    let Some((first, rest)) = changes.split_first() else {
        return Vec::new();
    };
    let mut prefix = first.value_path.clone();
    for change in rest {
        let shared = prefix
            .iter()
            .zip(&change.value_path)
            .take_while(|(left, right)| left == right)
            .count();
        prefix.truncate(shared);
    }
    prefix
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
    use super::{common_value_path, restore_component_values, set_json_value, SceneDocument};
    use crate::inspector::edit_tracker::{self, ComponentKey};
    use crate::inspector::type_inspector::InspectorResponse;
    use crate::scene_edit::SceneEdit;
    use crate::scene_history::InspectorValueChange;
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
        assert!(SceneDocument::new(Path::new("other-project"))
            .open_file(&mut reopened, root.scene_file()));
        assert_eq!(object_count(&reopened), 2);

        // Undoing past the save leaves the scene different from its file.
        document.undo(&mut game);
        assert!(document.is_dirty());
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
}
