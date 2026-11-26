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
use crate::widgets::{ThumbnailHandle, ThumbnailPriority};

pub struct SceneDocument {
    history: SceneHistory,
    /// The inspector's undo baselines for the selected object's components.
    inspector_edits: InspectorEditTracker,
    autosave: EditorSceneAutosave,
    /// Counts the changes to the undo history and the saved state.
    revision: u64,
    /// Where a save asks for the saved scene's new thumbnail.
    thumbnails: Option<ThumbnailHandle>,
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
            thumbnails: None,
        }
    }

    /// A document whose saves request the saved scene's thumbnail from
    /// `thumbnails`.
    pub(crate) fn with_thumbnails(mut self, thumbnails: ThumbnailHandle) -> Self {
        self.thumbnails = Some(thumbnails);
        self
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
                    "{} is outside every asset root ({roots}); the editor opens scenes only from an asset root, so save it inside one",
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
        let registered = game.assets.registries.assets.read().register_file(file);
        match registered {
            // The new file version is a new thumbnail key; ask for it ahead
            // of the thumbnails the browser asks for. A newer save replaces
            // the queued job, so repeated saves render once.
            Ok(asset_id) => {
                if let Some(thumbnails) = &self.thumbnails {
                    thumbnails.refresh_asset(
                        &game.assets.registries.assets.read(),
                        asset_id,
                        ThumbnailPriority::High,
                    );
                }
            }
            Err(error) => log::warn!(
                "Saved scene {} is not registered as an asset until the editor restarts: {error}",
                file.display()
            ),
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
mod tests;
