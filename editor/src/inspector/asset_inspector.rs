use engine::egui::Ui;
use engine::reflect_trait;
use engine::uuid::Uuid;

#[reflect_trait]
pub trait AssetInspector: Send + Sync {
    fn target_type_uuid(&self) -> Uuid;
    fn show_inspector(&self, _ui: &mut Ui, _asset_id: Uuid) {}
    fn has_context_menu(&self) -> bool {
        false
    }
    fn show_context_menu(&self, _ui: &mut Ui, _asset_id: Uuid) {}
}
