use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Prefab;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};

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

    fn show_context_menu(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        if ui.button("Import").clicked() {
            let asset_registry = game.assets.registries.assets.read();
            let prefab_meta = asset_registry.asset_meta_from_id(asset_id);
            let Ok(asset) = asset_registry.load_dyn_by_id(asset_id) else {
                ui.close_menu();
                return;
            };
            let Some(prefab_ref) = asset.try_downcast::<Prefab>() else {
                ui.close_menu();
                return;
            };
            let prefab = prefab_ref.read();
            drop(asset_registry);
            if let Some(game_object) = game
                .scenes
                .simulation_scene_mut()
                .instantiate_prefab(&prefab, None)
            {
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
            ui.close_menu();
        }
    }
}
