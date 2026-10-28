use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Prefab;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
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
    /// `edit`.
    pub fn import(edit: &mut SceneEdit, game: &mut GameContext, asset_id: Uuid) {
        let asset_registry = game.assets.registries.assets.read();
        let prefab_meta = asset_registry.asset_meta_from_id(asset_id);
        let Ok(asset) = asset_registry.load_dyn_by_id(asset_id) else {
            return;
        };
        let Some(prefab_ref) = asset.try_downcast::<Prefab>() else {
            return;
        };
        let prefab = prefab_ref.read();
        drop(asset_registry);
        match edit.scene_mut(game).instantiate_prefab(&prefab, None) {
            Ok(game_object) => {
                let scene = game.scenes.simulation_scene();
                log::info!(
                    "Instantiated prefab: prefab={} asset_id={} root_object={} ({})",
                    prefab_meta
                        .as_ref()
                        .map(|meta| meta.display_name.as_str())
                        .unwrap_or("<unknown>"),
                    asset_id,
                    scene.name(game_object),
                    scene.uuid(game_object)
                );
            }
            Err(err) => log::error!("Failed to instantiate prefab asset_id={asset_id}: {err}"),
        }
    }
}
