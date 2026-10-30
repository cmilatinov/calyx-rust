use calyx_engine::context::GameContext;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::Prefab;
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

use crate::asset_open::{
    AssetOpenContext, AssetOpenOutcome, AssetOpener, ReflectAssetOpener, OPEN_DURING_PLAY,
};
use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
use crate::panel::{Panel, PanelViewport};
use crate::prefab_instances;
use crate::prefab_mode::PrefabOpenRequest;
use crate::scene_edit::SceneEdit;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector, AssetOpener)]
pub struct PrefabInspector;

impl AssetInspector for PrefabInspector {
    fn target_type_uuid(&self) -> Uuid {
        Prefab::type_uuid()
    }

    fn has_context_menu(&self) -> bool {
        true
    }

    fn show_context_menu(
        &self,
        ui: &mut Ui,
        game: &mut GameContext,
        asset_id: Uuid,
    ) -> AssetInspectorAction {
        if ui.button("Import").clicked() {
            ui.close_menu();
            return AssetInspectorAction::ImportPrefab(asset_id);
        }
        let has_instances = game
            .scenes
            .simulation_scene()
            .linked_prefabs()
            .contains(&asset_id);
        let update_response = ui
            .add_enabled(has_instances, egui::Button::new("Update Instances"))
            .on_hover_text("Update this prefab's instances in the open scene from the saved prefab")
            .on_disabled_hover_text("The open scene has no linked instances of this prefab");
        if update_response.clicked() {
            ui.close_menu();
            return AssetInspectorAction::UpdatePrefabInstances(asset_id);
        }
        AssetInspectorAction::None
    }
}

/// A prefab opens in prefab mode, asking first about the open prefab's
/// unsaved changes.
impl AssetOpener for PrefabInspector {
    fn target_type_uuid(&self) -> Uuid {
        Prefab::type_uuid()
    }

    fn open_unavailable_reason(
        &self,
        editor: &AssetOpenContext<'_>,
        asset_id: Uuid,
    ) -> Option<&'static str> {
        if editor.in_play_mode() {
            return Some(OPEN_DURING_PLAY);
        }
        if !editor.assemblies_loaded() {
            return Some("Build project assemblies before opening prefabs");
        }
        let editable = editor
            .game
            .assets
            .registries
            .assets
            .read()
            .asset_meta_from_id(asset_id)
            .and_then(|meta| meta.path)
            .is_some_and(|path| path.extension().is_some_and(|ext| ext == "cxprefab"));
        (!editable).then_some("Prefabs imported from model files cannot be edited")
    }

    fn open(&self, editor: &mut AssetOpenContext<'_>, asset_id: Uuid) -> AssetOpenOutcome {
        match editor.prefab_mode_mut().request_open(asset_id) {
            Ok(PrefabOpenRequest::Opened) => {
                editor.focus_panel(PanelViewport::name());
                AssetOpenOutcome::Opened
            }
            Ok(PrefabOpenRequest::Prompted) => AssetOpenOutcome::Prompted,
            Err(error) => {
                log::error!("{error}");
                AssetOpenOutcome::Refused(error)
            }
        }
    }
}

impl PrefabInspector {
    /// Instantiates the prefab asset `asset_id` at the scene root through
    /// `edit`, linked to the prefab.
    pub fn import(edit: &mut SceneEdit, game: &mut GameContext, asset_id: Uuid) {
        if let Err(err) = prefab_instances::instantiate_prefab(edit, game, asset_id) {
            log::error!("{err}");
        }
    }
}
