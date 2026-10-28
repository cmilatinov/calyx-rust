use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Prefab;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
use crate::prefab_instances;
use crate::project_manager::ProjectAssemblyStatus;
use crate::scene_edit::SceneEdit;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
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
        let open_response = match Self::open_unavailable_reason(game, asset_id) {
            Some(reason) => ui
                .add_enabled(false, egui::Button::new("Open Prefab"))
                .on_disabled_hover_text(reason),
            None => ui
                .button("Open Prefab")
                .on_hover_text("Edit the prefab asset in prefab mode"),
        };
        if open_response.clicked() {
            ui.close_menu();
            return AssetInspectorAction::OpenPrefab(asset_id);
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

impl PrefabInspector {
    /// Why the prefab asset `asset_id` cannot open in prefab mode now, if it
    /// cannot.
    fn open_unavailable_reason(game: &GameContext, asset_id: Uuid) -> Option<&'static str> {
        let assemblies_loaded = game
            .resources
            .resource::<engine::core::Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded());
        if !assemblies_loaded {
            return Some("Build project assemblies before opening prefabs");
        }
        let editable = game
            .assets
            .registries
            .assets
            .read()
            .asset_meta_from_id(asset_id)
            .and_then(|meta| meta.path)
            .is_some_and(|path| path.extension().is_some_and(|ext| ext == "cxprefab"));
        (!editable).then_some("Prefabs imported from model files cannot be edited")
    }

    /// Instantiates the prefab asset `asset_id` at the scene root through
    /// `edit`, linked to the prefab.
    pub fn import(edit: &mut SceneEdit, game: &mut GameContext, asset_id: Uuid) {
        if let Err(err) = prefab_instances::instantiate_prefab(edit, game, asset_id) {
            log::error!("{err}");
        }
    }
}
