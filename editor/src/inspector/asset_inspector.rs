use engine::egui::Ui;
use engine::uuid::Uuid;
use reflect::reflect_trait;

#[reflect_trait]
pub trait AssetInspector {
    fn target_type_uuid(&self) -> Uuid;
    fn show_inspector(&self, _ui: &mut Ui, _asset_id: Uuid) {}
    fn show_context_menu(&self, _ui: &mut Ui, _asset_id: Uuid) {}
}
