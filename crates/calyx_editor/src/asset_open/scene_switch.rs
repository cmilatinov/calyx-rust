//! Switching the open scene: the guards every scene open goes through, and the
//! prompt about the open scene's unsaved changes, modelled on prefab mode's
//! unsaved-prefab prompt.

use std::path::PathBuf;

use uuid::Uuid;

use super::{AssetOpenContext, AssetOpenOutcome, OPEN_DURING_PLAY};
use crate::panel::{Panel, PanelViewport};
use crate::prefab_mode::CLOSE_PREFAB_FIRST;
use crate::EditorAppState;

/// The scene a scene open loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SceneOpenTarget {
    /// The registered scene asset with this id.
    Asset(Uuid),
    /// The scene file at this path, as File > Open picks it.
    File(PathBuf),
}

/// The user's answer to the unsaved-scene prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneSwitchAnswer {
    /// Save the open scene, then open the other one.
    Save,
    /// Open the other scene; the open scene's changes stay in its recovery
    /// autosave.
    Discard,
    /// Keep the open scene.
    Cancel,
}

/// The scene open waiting on the user's answer about the open scene's unsaved
/// changes, if one is.
#[derive(Debug, Default)]
pub(crate) struct SceneSwitch {
    pending: Option<SceneOpenTarget>,
}

impl SceneSwitch {
    /// The scene open waiting on the prompt, if one is.
    pub(crate) fn pending(&self) -> Option<&SceneOpenTarget> {
        self.pending.as_ref()
    }
}

impl AssetOpenContext<'_> {
    /// Why the open scene cannot be switched now, if it cannot.
    pub(crate) fn scene_switch_refusal(&self) -> Option<&'static str> {
        if self.in_play_mode() {
            Some(OPEN_DURING_PLAY)
        } else if self.prefab_mode.is_open() {
            Some(CLOSE_PREFAB_FIRST)
        } else {
            None
        }
    }

    /// Refuses with a logged reason when the open scene cannot be switched.
    fn refuse_scene_switch(&mut self) -> Option<AssetOpenOutcome> {
        let reason = self.scene_switch_refusal()?;
        // Prefab mode logs which prefab keeps the scene set aside.
        if !self.prefab_mode.refuse_scene_change() {
            log::warn!("Cannot open a scene: {reason}");
        }
        self.scene_switch.pending = None;
        Some(AssetOpenOutcome::Refused(reason.to_string()))
    }

    /// Opens the scene `target`, first asking about the open scene's unsaved
    /// changes, if it has any. Refused during play and while a prefab is
    /// open.
    pub(crate) fn request_scene_open(&mut self, target: SceneOpenTarget) -> AssetOpenOutcome {
        if let Some(refused) = self.refuse_scene_switch() {
            return refused;
        }
        if self.scene_document.is_dirty() {
            self.scene_switch.pending = Some(target);
            return AssetOpenOutcome::Prompted;
        }
        self.open_scene_now(target)
    }

    /// Takes the user's answer to the unsaved-scene prompt. Save writes the
    /// open scene to its file, or to the file `pick_save_file` chooses when
    /// it has none, and opens the other scene only once that worked.
    pub(crate) fn answer_scene_prompt(
        &mut self,
        answer: SceneSwitchAnswer,
        pick_save_file: impl FnOnce() -> Option<PathBuf>,
    ) -> AssetOpenOutcome {
        let Some(target) = self.scene_switch.pending.take() else {
            return AssetOpenOutcome::Refused("no scene open is waiting on an answer".into());
        };
        // Play or prefab mode may have started since the prompt opened.
        if let Some(refused) = self.refuse_scene_switch() {
            return refused;
        }
        match answer {
            SceneSwitchAnswer::Cancel => {
                return AssetOpenOutcome::Refused("the scene open was cancelled".into())
            }
            SceneSwitchAnswer::Save => {
                let file = self
                    .game
                    .scenes
                    .current_scene_meta()
                    .file
                    .clone()
                    .or_else(pick_save_file);
                let Some(file) = file else {
                    log::info!("Kept the open scene: it was not saved");
                    return AssetOpenOutcome::Refused("the open scene was not saved".into());
                };
                // The scene document logs why the save failed.
                if let Err(error) = self.scene_document.save(self.game, file) {
                    log::info!("Kept the open scene: it failed to save");
                    return AssetOpenOutcome::Refused(format!(
                        "the open scene failed to save: {error}"
                    ));
                }
            }
            SceneSwitchAnswer::Discard => {}
        }
        self.open_scene_now(target)
    }

    /// Opens the scene `target` through the scene document, as File > Open
    /// does.
    fn open_scene_now(&mut self, target: SceneOpenTarget) -> AssetOpenOutcome {
        let opened = match target {
            SceneOpenTarget::Asset(asset_id) => self.scene_document.open_asset(self.game, asset_id),
            SceneOpenTarget::File(file) => self.scene_document.open_file(self.game, file),
        };
        if opened {
            self.focus_panel(PanelViewport::name());
            AssetOpenOutcome::Opened
        } else {
            AssetOpenOutcome::Refused("the scene did not open; see the editor log".into())
        }
    }
}

/// Asks what to do with the open scene's unsaved changes before the scene
/// open waiting on the answer: save them, don't save them, or cancel.
pub(crate) fn show_unsaved_scene_prompt(ctx: &egui::Context, state: &mut EditorAppState) {
    if state.scene_switch.pending().is_none() {
        return;
    }
    let label = state.active_scene.scene_label();
    let mut answer = None;
    egui::Modal::new(egui::Id::new("scene_unsaved_changes")).show(ctx, |ui| {
        ui.heading("Unsaved Scene Changes");
        ui.separator();
        ui.label(format!("{label} has unsaved changes."));
        ui.label("Save them before opening another scene?");
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                answer = Some(SceneSwitchAnswer::Save);
            }
            if ui.button("Don't Save").clicked() {
                answer = Some(SceneSwitchAnswer::Discard);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(SceneSwitchAnswer::Cancel);
            }
        });
    });
    if let Some(answer) = answer {
        state.answer_scene_prompt(answer, crate::EditorApp::pick_scene_save_file);
    }
}
