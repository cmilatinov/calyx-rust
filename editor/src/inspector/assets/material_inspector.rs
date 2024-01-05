use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use engine::assets::material::Material;
use engine::egui::Ui;
use engine::uuid::Uuid;
use reflect::ReflectDefault;
use reflect::{Reflect, TypeUuid};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct MaterialInspector;

impl AssetInspector for MaterialInspector {
    fn target_type_uuid(&self) -> Uuid {
        Material::type_uuid()
    }
    fn show_inspector(&self, ui: &mut Ui, asset_id: Uuid) {}
    fn show_context_menu(&self, ui: &mut Ui, asset_id: Uuid) {}
}
