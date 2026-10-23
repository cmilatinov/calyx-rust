use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Prefab;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
use crate::scene_edit::SceneEdit;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
#[repr(C)]
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
        _game: &mut GameContext,
        asset_id: Uuid,
    ) -> AssetInspectorAction {
        if ui.button("Import").clicked() {
            ui.close_menu();
            return AssetInspectorAction::ImportPrefab(asset_id);
        }
        AssetInspectorAction::None
    }
}

impl PrefabInspector {
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
