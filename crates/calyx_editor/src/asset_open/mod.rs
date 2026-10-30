//! Opening assets: the one dispatch that double-click and Enter in the content
//! browser, its context menu's "Open" and `calyx_ctl asset open` all go
//! through.
//!
//! Each asset type that opens somewhere registers an [`AssetOpener`] next to
//! its asset inspector, reflected like the inspector, and
//! [`AssetOpenerRegistry`] finds it by asset type UUID. A type without one is
//! opened by selecting it, so the inspector shows it.
//!
//! Scene opens wait on the unsaved-scene prompt (`scene_switch.rs`) when the
//! open scene has unsaved changes. Scene and prefab openers refuse during
//! play, while the authoring scene cannot change under the simulation.

use std::collections::HashMap;
use std::path::Path;

use calyx_engine::context::GameContext;
use calyx_engine::core::Ref;
use calyx_engine::reflect::type_registry::TypeRegistry;
use calyx_engine::reflect::ReflectDefault;
use calyx_engine::{reflect_trait, type_uuids};
use uuid::Uuid;

use crate::panel::{Panel, PanelViewport};
use crate::prefab_mode::{PrefabMode, PrefabModeMut, PrefabPromptAnswer, PrefabPromptOutcome};
use crate::scene_document::SceneDocument;
use crate::selection::{Selection, SelectionType};
use crate::{EditorAppState, ProjectAssemblyStatus};

mod scene_switch;
#[cfg(test)]
mod tests;

pub(crate) use self::scene_switch::{
    show_unsaved_scene_prompt, SceneOpenTarget, SceneSwitch, SceneSwitchAnswer,
};

/// Why scene and prefab opens are refused during play.
pub(crate) const OPEN_DURING_PLAY: &str = "Stop play mode before opening a scene or prefab";

/// What an asset open did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AssetOpenOutcome {
    /// The asset opened where it belongs: as the scene, in prefab mode, in
    /// its panel or in the OS default app.
    Opened,
    /// The asset is selected, so the inspector shows it. What a type without
    /// an opener gets.
    Selected,
    /// The open waits on the user's answer about unsaved changes.
    Prompted,
    /// The open was refused or failed, for the reason given, which is also
    /// logged.
    Refused(String),
}

/// How one asset type opens. Implemented next to the type's asset inspector
/// and registered with `#[reflect(Default, AssetOpener)]`.
#[reflect_trait]
pub(crate) trait AssetOpener: Send + Sync {
    /// The asset type this opener opens.
    fn target_type_uuid(&self) -> Uuid;

    /// Why the asset `asset_id` cannot open now, if it cannot. The context
    /// menu disables "Open" with it as hover text, and the dispatch refuses
    /// with it.
    fn open_unavailable_reason(
        &self,
        editor: &AssetOpenContext<'_>,
        asset_id: Uuid,
    ) -> Option<&'static str> {
        let _ = (editor, asset_id);
        None
    }

    /// Opens the asset `asset_id`.
    fn open(&self, editor: &mut AssetOpenContext<'_>, asset_id: Uuid) -> AssetOpenOutcome;
}

/// The asset openers, by the asset type UUID they open.
#[derive(Default)]
pub(crate) struct AssetOpenerRegistry {
    openers: HashMap<Uuid, Box<dyn AssetOpener>>,
}

impl AssetOpenerRegistry {
    /// Collects every reflected type that registers an [`AssetOpener`].
    pub(crate) fn new(registry: &TypeRegistry) -> Self {
        let mut openers = HashMap::new();
        for type_id in registry.all_of(type_uuids!(ReflectDefault, ReflectAssetOpener)) {
            let (Some(meta_default), Some(meta_opener)) = (
                registry.trait_meta::<ReflectDefault>(type_id),
                registry.trait_meta::<ReflectAssetOpener>(type_id),
            ) else {
                continue;
            };
            let Ok(opener) = meta_opener.get_boxed(meta_default.default()) else {
                continue;
            };
            openers.insert(opener.target_type_uuid(), opener);
        }
        Self { openers }
    }

    /// The opener of the asset type `type_uuid`, if it has one.
    pub(crate) fn lookup(&self, type_uuid: Uuid) -> Option<&dyn AssetOpener> {
        self.openers.get(&type_uuid).map(|opener| opener.as_ref())
    }

    /// The opener of the asset `asset_id`'s type, if it has one.
    fn opener_of(&self, game: &GameContext, asset_id: Uuid) -> Option<&dyn AssetOpener> {
        let type_uuid = game
            .assets
            .registries
            .assets
            .read()
            .asset_meta_from_id(asset_id)?
            .type_uuid;
        self.lookup(type_uuid)
    }

    /// Why the asset `asset_id` cannot open now, if it cannot. Assets without
    /// an opener can always be selected.
    pub(crate) fn open_unavailable_reason(
        &self,
        editor: &AssetOpenContext<'_>,
        asset_id: Uuid,
    ) -> Option<&'static str> {
        self.opener_of(editor.game, asset_id)?
            .open_unavailable_reason(editor, asset_id)
    }

    /// Opens the asset `asset_id` through its type's opener, or selects it
    /// when its type has none.
    pub(crate) fn open(
        &self,
        editor: &mut AssetOpenContext<'_>,
        asset_id: Uuid,
    ) -> AssetOpenOutcome {
        let Some(opener) = self.opener_of(editor.game, asset_id) else {
            editor.select(asset_id);
            return AssetOpenOutcome::Selected;
        };
        if let Some(reason) = opener.open_unavailable_reason(editor, asset_id) {
            log::warn!(
                "Cannot open asset {}: {reason}",
                editor.asset_label(asset_id)
            );
            return AssetOpenOutcome::Refused(reason.to_string());
        }
        opener.open(editor, asset_id)
    }
}

/// The editor state an asset open changes, as separate borrows so the
/// dispatch can run beside the registry that holds the openers.
pub(crate) struct AssetOpenContext<'a> {
    pub(crate) game: &'a mut GameContext,
    pub(crate) scene_document: &'a mut SceneDocument,
    pub(crate) prefab_mode: &'a mut PrefabMode,
    pub(crate) selection: &'a mut Selection,
    pub(crate) project_path: &'a Path,
    pub(crate) scene_switch: &'a mut SceneSwitch,
    /// The panel to bring to the front, by name, before the next panels draw.
    pub(crate) focus_panel: &'a mut Option<&'static str>,
}

impl AssetOpenContext<'_> {
    /// Selects the asset `asset_id`, so the inspector shows it.
    pub(crate) fn select(&mut self, asset_id: Uuid) {
        *self.selection = Selection::from_id(SelectionType::Asset, asset_id);
    }

    /// Asks for the panel named `name` to come to the front.
    pub(crate) fn focus_panel(&mut self, name: &'static str) {
        *self.focus_panel = Some(name);
    }

    /// Prefab mode with the editor state it opens prefabs in.
    pub(crate) fn prefab_mode_mut(&mut self) -> PrefabModeMut<'_> {
        self.prefab_mode.with(
            self.game,
            self.scene_document,
            self.selection,
            self.project_path,
        )
    }

    /// Takes the user's answer to the unsaved-prefab prompt, bringing the
    /// scene view to the front when the prefab that waited on it opens, as
    /// an immediate prefab open does.
    pub(crate) fn answer_prefab_prompt(
        &mut self,
        answer: PrefabPromptAnswer,
    ) -> PrefabPromptOutcome {
        let outcome = self.prefab_mode_mut().answer_prompt(answer);
        if outcome == PrefabPromptOutcome::Opened {
            self.focus_panel(PanelViewport::name());
        }
        outcome
    }

    /// Whether a simulation copy of the scene exists, playing or paused.
    pub(crate) fn in_play_mode(&self) -> bool {
        self.game.scenes.has_simulation_scene()
    }

    /// Whether the project assemblies are loaded, without which project
    /// components would be dropped from an opened scene or prefab.
    pub(crate) fn assemblies_loaded(&self) -> bool {
        self.game
            .resources
            .resource::<Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded())
    }

    /// The asset's path for log messages, else its id.
    pub(crate) fn asset_label(&self, asset_id: Uuid) -> String {
        self.game
            .assets
            .registries
            .assets
            .read()
            .asset_meta_from_id(asset_id)
            .and_then(|meta| meta.path)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| asset_id.to_string())
    }
}

impl EditorAppState {
    /// The dispatch's view of the editor state, beside the opener registry.
    fn asset_open_parts(&mut self) -> (&AssetOpenerRegistry, AssetOpenContext<'_>) {
        (
            &self.asset_openers,
            AssetOpenContext {
                game: &mut self.game,
                scene_document: &mut self.scene_document,
                prefab_mode: &mut self.prefab_mode,
                selection: &mut self.selection,
                project_path: &self.project_path,
                scene_switch: &mut self.scene_switch,
                focus_panel: &mut self.focus_panel,
            },
        )
    }

    /// Opens the asset `asset_id` where it belongs, or selects it when its
    /// type has no opener.
    pub(crate) fn open_asset(&mut self, asset_id: Uuid) -> AssetOpenOutcome {
        let (openers, mut editor) = self.asset_open_parts();
        openers.open(&mut editor, asset_id)
    }

    /// Why the asset `asset_id` cannot open now, if it cannot.
    pub(crate) fn asset_open_unavailable_reason(&mut self, asset_id: Uuid) -> Option<&'static str> {
        let (openers, editor) = self.asset_open_parts();
        openers.open_unavailable_reason(&editor, asset_id)
    }

    /// Opens the scene `target`, as File > Open does: refused during play
    /// and while a prefab is open, and asking about the open scene's unsaved
    /// changes first.
    pub(crate) fn request_scene_open(&mut self, target: SceneOpenTarget) -> AssetOpenOutcome {
        let (_, mut editor) = self.asset_open_parts();
        editor.request_scene_open(target)
    }

    /// Answers the unsaved-scene prompt waiting on the user, if one is.
    /// `pick_save_file` chooses where to save a scene that has no file yet.
    pub(crate) fn answer_scene_prompt(
        &mut self,
        answer: SceneSwitchAnswer,
        pick_save_file: impl FnOnce() -> Option<std::path::PathBuf>,
    ) -> AssetOpenOutcome {
        let (_, mut editor) = self.asset_open_parts();
        editor.answer_scene_prompt(answer, pick_save_file)
    }

    /// Answers the unsaved-prefab prompt waiting on the user, if one is.
    pub(crate) fn answer_prefab_prompt(
        &mut self,
        answer: PrefabPromptAnswer,
    ) -> PrefabPromptOutcome {
        let (_, mut editor) = self.asset_open_parts();
        editor.answer_prefab_prompt(answer)
    }
}
