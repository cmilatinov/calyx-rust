use engine::assets::Asset;
use engine::core::Ref;
use engine::egui::Ui;
use engine::reflect_trait;
use engine::uuid::Uuid;

#[reflect_trait]
pub trait AssetInspector {
    fn target_type_uuid(&self) -> Uuid;
    fn show_inspector(&self, _ui: &mut Ui, _asset: Ref<dyn Asset>) {}
    fn show_context_menu(&self, _ui: &mut Ui, _asset: Ref<dyn Asset>) {}
}
