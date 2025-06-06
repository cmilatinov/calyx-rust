use egui::Ui;
use engine::context::GameContext;
use engine::reflect_trait;
use uuid::Uuid;

#[reflect_trait]
pub trait AssetInspector: Send + Sync {
    fn target_type_uuid(&self) -> Uuid;
    fn show_inspector(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let _ = (ui, game, asset_id);
    }
    fn has_context_menu(&self) -> bool {
        false
    }
    fn show_context_menu(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let _ = (ui, game, asset_id);
    }
}
