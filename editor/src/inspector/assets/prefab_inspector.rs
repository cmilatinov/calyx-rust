use engine::assets::Asset;
use engine::core::Ref;
use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::{Prefab, SceneManager};
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
    fn show_context_menu(&self, ui: &mut Ui, asset: Ref<dyn Asset>) {
        if let Some(prefab_ref) = asset.try_downcast::<Prefab>() {
            if ui.button("Import").clicked() {
                let prefab = prefab_ref.read();
                SceneManager::get_mut()
                    .simulation_scene_mut()
                    .instantiate_prefab(&prefab, None);
                ui.close_menu();
            }
        }
    }
}
