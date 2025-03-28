use engine::context::GameContext;
use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Prefab;
use engine::utils::TypeUuid;
use engine::uuid::Uuid;

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};

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

    fn show_context_menu(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        if ui.button("Import").clicked() {
            let Ok(asset) = game.assets.asset_registry.read().load_dyn_by_id(asset_id) else {
                return;
            };
            if let Some(prefab_ref) = asset.try_downcast::<Prefab>() {
                let prefab = prefab_ref.read();
                game.scenes
                    .simulation_scene_mut()
                    .instantiate_prefab(&prefab, None);
            }
            ui.close_menu();
        }
    }
}
