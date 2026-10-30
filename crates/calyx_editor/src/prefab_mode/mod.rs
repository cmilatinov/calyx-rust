//! Prefab mode: a `.cxprefab` asset opened as the editor's authoring scene,
//! isolated from the scene that was open.
//!
//! Opening a prefab sets the open scene aside and puts a scene built from the
//! prefab's data in the authoring slot under a document of its own (see
//! `editing.rs`). The panels, the undo history and the remote commands then
//! edit the prefab like any scene. Saving writes the prefab asset only.
//! Closing the prefab puts the scene that was set aside back as it was.
//!
//! Scene instances of the prefab are copies made when they were instantiated,
//! so editing or saving the prefab never changes them. Applying prefab edits
//! to existing instances is a separate workflow.
//!
//! While a prefab is open the scene view shows [`PREFAB_MODE_BACKDROP`] and
//! lights the prefab with a studio environment (`environment.rs`), and the
//! editor shows a banner and asks about unsaved changes before closing the
//! prefab (`ui.rs`).

use std::path::Path;

use calyx_engine::assets::skybox::Skybox;
use calyx_engine::context::GameContext;
use calyx_engine::core::Ref;
use calyx_engine::render::SceneRendererOptions;
use egui::Color32;
use uuid::Uuid;

use self::editing::PrefabEditing;
use crate::scene_document::SceneDocument;
use crate::selection::Selection;
use crate::{EditorAppState, ProjectAssemblyStatus};

mod editing;
mod environment;
mod ui;

pub(crate) use self::ui::{
    show_banner, show_unsaved_changes_prompt, CLOSE_PREFAB_FIRST, PLAY_IN_PREFAB_MODE,
};

/// The scene view's clear color, behind the grid where nothing is drawn. The
/// renderer takes it as linear HDR color and tone maps it with ACES at 0 EV,
/// so this near-black shows as dark gray (about sRGB 49).
pub(crate) const SCENE_VIEW_BACKDROP: Color32 = Color32::from_rgb(21, 21, 21);
/// The scene view's clear color in prefab mode: a muted blue-gray in the
/// banner's hue, so prefab mode reads at a glance while the grid, gizmos and
/// objects stay legible. Tone mapped like [`SCENE_VIEW_BACKDROP`], so it
/// shows as a lighter slate blue (about sRGB 41, 61, 90).
const PREFAB_MODE_BACKDROP: Color32 = Color32::from_rgb(17, 27, 47);

/// The scene view's clear color: blue-gray in prefab mode, else the normal
/// backdrop.
fn scene_view_backdrop(prefab_mode: bool) -> Color32 {
    if prefab_mode {
        PREFAB_MODE_BACKDROP
    } else {
        SCENE_VIEW_BACKDROP
    }
}

/// The label naming the prefab asset `asset_label` as the open document, in
/// the window title and atop the scene hierarchy.
fn document_label(asset_label: &str) -> String {
    format!("Prefab: {asset_label}")
}

/// What the editor does once the user answers the prompt about an open
/// prefab's unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefabPrompt {
    /// Return to the scene.
    Close,
    /// Open another prefab asset.
    Open(Uuid),
    /// Close the editor window.
    Quit,
}

/// The user's answer to the unsaved-prefab prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefabPromptAnswer {
    /// Save the open prefab, then carry on.
    Save,
    /// Carry on and drop the open prefab's changes.
    Discard,
    /// Keep the open prefab.
    Cancel,
}

/// What answering the unsaved-prefab prompt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefabPromptOutcome {
    /// The open prefab stayed open: the answer was Cancel, its save failed,
    /// or no prompt was waiting.
    Kept,
    /// The prefab closed and the scene is back.
    Closed,
    /// The prefab that waited on the prompt is open.
    Opened,
    /// The editor window may close.
    Quit,
}

/// The editor's prefab mode: the prefab open in it, if one is, and what waits
/// on the user's answer about its unsaved changes.
#[derive(Default)]
pub(crate) struct PrefabMode {
    /// The prefab open in prefab mode, if one is.
    prefab: Option<PrefabEditing>,
    /// What waits on the user's answer to the unsaved-prefab prompt.
    prompt: Option<PrefabPrompt>,
    /// The studio HDRI that lights prefab mode, loaded when a prefab first
    /// opens.
    environment: Option<Ref<Skybox>>,
    /// Set once closing the window no longer waits on the unsaved-prefab
    /// prompt: the user answered it, or a remote client asked to shut down.
    close_confirmed: bool,
}

impl PrefabMode {
    /// Whether a prefab is open.
    pub(crate) fn is_open(&self) -> bool {
        self.prefab.is_some()
    }

    /// The prefab open in prefab mode, if one is.
    pub(crate) fn prefab(&self) -> Option<&PrefabEditing> {
        self.prefab.as_ref()
    }

    /// The label naming the open prefab as the open document, `None` outside
    /// prefab mode.
    pub(crate) fn document_label(&self) -> Option<String> {
        self.prefab
            .as_ref()
            .map(|prefab| document_label(prefab.label()))
    }

    /// Logs and returns `true` while a prefab is open, which keeps the scene
    /// set aside until the prefab closes.
    pub(crate) fn refuse_scene_change(&self) -> bool {
        let Some(prefab) = &self.prefab else {
            return false;
        };
        log::warn!(
            "Close the prefab {} before opening or saving a scene",
            prefab.label()
        );
        true
    }

    /// Lets the window close without asking about the open prefab's unsaved
    /// changes.
    pub(crate) fn confirm_close(&mut self) {
        self.close_confirmed = true;
    }

    /// Sets the scene view's backdrop and lighting for prefab mode, or back
    /// to the scene's when no prefab is open.
    pub(crate) fn dress_scene_view(&self, options: &mut SceneRendererOptions) {
        options.clear_color = scene_view_backdrop(self.is_open());
        options.environment =
            environment::scene_view_environment(self.is_open(), self.environment.as_ref());
    }
}

/// Prefab mode with the editor state it opens, saves and closes prefabs in.
pub(crate) struct PrefabModeMut<'a> {
    mode: &'a mut PrefabMode,
    game: &'a mut GameContext,
    scene_document: &'a mut SceneDocument,
    selection: &'a mut Selection,
    project_path: &'a Path,
}

impl EditorAppState {
    /// Prefab mode with the editor state it opens, saves and closes prefabs
    /// in.
    pub(crate) fn prefab_mode_mut(&mut self) -> PrefabModeMut<'_> {
        self.prefab_mode.with(
            &mut self.game,
            &mut self.scene_document,
            &mut self.selection,
            &self.project_path,
        )
    }
}

impl PrefabMode {
    /// Prefab mode with the editor state it opens, saves and closes prefabs
    /// in, for callers that hold that state as separate borrows.
    pub(crate) fn with<'a>(
        &'a mut self,
        game: &'a mut GameContext,
        scene_document: &'a mut SceneDocument,
        selection: &'a mut Selection,
        project_path: &'a Path,
    ) -> PrefabModeMut<'a> {
        PrefabModeMut {
            mode: self,
            game,
            scene_document,
            selection,
            project_path,
        }
    }
}

/// What [`PrefabModeMut::request_open`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrefabOpenRequest {
    /// The prefab is open in prefab mode.
    Opened,
    /// The unsaved-prefab prompt waits on the user's answer first.
    Prompted,
}

impl PrefabModeMut<'_> {
    /// Opens the prefab asset `asset_id` in prefab mode, closing the open
    /// prefab first. Refuses while project assemblies are not loaded, since
    /// the prefab's project components would be dropped and lost on save,
    /// and while the open prefab has unsaved changes.
    pub(crate) fn open(&mut self, asset_id: Uuid) -> Result<(), String> {
        let assemblies_loaded = self
            .game
            .resources
            .resource::<Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded());
        if !assemblies_loaded {
            return Err("build project assemblies before opening a prefab".into());
        }
        if let Some(prefab) = &self.mode.prefab {
            if prefab.asset_id() == asset_id {
                return Ok(());
            }
            if self.scene_document.is_dirty() {
                return Err(format!(
                    "the open prefab {} has unsaved changes; save or close it first",
                    prefab.label()
                ));
            }
            self.close();
        }
        let prefab = PrefabEditing::open(
            self.game,
            self.scene_document,
            self.selection,
            self.project_path,
            asset_id,
        )?;
        self.mode.prefab = Some(prefab);
        self.load_environment();
        Ok(())
    }

    /// Loads the studio HDRI that lights prefab mode, once. Without it the
    /// prefab renders with its own lights only.
    fn load_environment(&mut self) {
        if self.mode.environment.is_some() {
            return;
        }
        match self
            .game
            .assets
            .registries
            .assets
            .read()
            .load::<Skybox>(environment::PREFAB_ENVIRONMENT_ASSET)
        {
            Ok(skybox) => self.mode.environment = Some(skybox),
            Err(error) => log::warn!("Prefab mode lighting is unavailable: {error}"),
        }
    }

    /// Saves the open prefab to its asset file.
    pub(crate) fn save(&mut self) -> Result<(), String> {
        let prefab = self.mode.prefab.as_ref().ok_or("no prefab is open")?;
        prefab.save(self.game, self.scene_document)
    }

    /// Closes the open prefab, dropping its unsaved changes, and returns to
    /// the scene. `false` when no prefab is open.
    pub(crate) fn close(&mut self) -> bool {
        let Some(prefab) = self.mode.prefab.take() else {
            return false;
        };
        self.mode.prompt = None;
        prefab.close(self.game, self.scene_document, self.selection);
        true
    }

    /// Opens the prefab asset `asset_id`, first asking what to do with the
    /// open prefab's unsaved changes, if it has any.
    pub(crate) fn request_open(&mut self, asset_id: Uuid) -> Result<PrefabOpenRequest, String> {
        let open_prefab_is_dirty =
            self.mode.prefab.as_ref().is_some_and(|prefab| {
                prefab.asset_id() != asset_id && self.scene_document.is_dirty()
            });
        if open_prefab_is_dirty {
            self.mode.prompt = Some(PrefabPrompt::Open(asset_id));
            return Ok(PrefabOpenRequest::Prompted);
        }
        self.open(asset_id)
            .map(|()| PrefabOpenRequest::Opened)
            .map_err(|error| format!("Failed to open prefab: {error}"))
    }

    /// Takes the user's answer to the unsaved-prefab prompt and carries out
    /// what waited on it. Save writes the open prefab first, and keeps it
    /// open when that fails.
    pub(crate) fn answer_prompt(&mut self, answer: PrefabPromptAnswer) -> PrefabPromptOutcome {
        let Some(action) = self.mode.prompt.take() else {
            return PrefabPromptOutcome::Kept;
        };
        match answer {
            PrefabPromptAnswer::Cancel => return PrefabPromptOutcome::Kept,
            PrefabPromptAnswer::Save => {
                if let Err(error) = self.save() {
                    log::error!("{error}");
                    return PrefabPromptOutcome::Kept;
                }
            }
            PrefabPromptAnswer::Discard => {}
        }
        match action {
            PrefabPrompt::Close => {
                self.close();
                PrefabPromptOutcome::Closed
            }
            PrefabPrompt::Open(asset_id) => {
                self.close();
                match self.open(asset_id) {
                    Ok(()) => PrefabPromptOutcome::Opened,
                    Err(error) => {
                        log::error!("Failed to open prefab: {error}");
                        PrefabPromptOutcome::Closed
                    }
                }
            }
            PrefabPrompt::Quit => {
                self.mode.confirm_close();
                PrefabPromptOutcome::Quit
            }
        }
    }

    /// Closes the open prefab, first asking what to do with its unsaved
    /// changes, if it has any.
    pub(crate) fn request_close(&mut self) {
        if self.mode.prefab.is_none() {
            return;
        }
        if self.scene_document.is_dirty() {
            self.mode.prompt = Some(PrefabPrompt::Close);
        } else {
            self.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{document_label, scene_view_backdrop, PREFAB_MODE_BACKDROP, SCENE_VIEW_BACKDROP};
    use crate::ActiveSceneState;

    #[test]
    fn prefab_mode_title_names_the_prefab_asset() {
        let mut state = ActiveSceneState::default();

        state.set_scene(Some("scene".into()), None);
        assert_eq!(
            state.title(
                "Sandbox",
                Some(&document_label("prefabs/crate.cxprefab")),
                true
            ),
            "Calyx - Sandbox - Prefab: prefabs/crate.cxprefab *"
        );
    }

    #[test]
    fn only_prefab_mode_turns_the_scene_view_backdrop_blue() {
        assert_eq!(scene_view_backdrop(true), PREFAB_MODE_BACKDROP);
        assert_eq!(scene_view_backdrop(false), SCENE_VIEW_BACKDROP);
        let [r, g, b, _] = PREFAB_MODE_BACKDROP.to_array();
        assert!(b > g && g > r, "the prefab backdrop is blue");
    }
}
