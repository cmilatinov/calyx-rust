use super::inspector_registry::InspectorRegistry;
use calyx_engine::context::GameContext;
use calyx_engine::reflect_trait;
use egui::Ui;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetInspectorAction {
    None,
    /// Instantiate the prefab asset into the scene, as one undo step.
    ImportPrefab(Uuid),
    /// Update the open scene's instances of the prefab asset from it, as one
    /// undo step.
    UpdatePrefabInstances(Uuid),
}

#[reflect_trait]
pub trait AssetInspector: Send + Sync {
    fn target_type_uuid(&self) -> Uuid;
    /// Draws the editing widgets for the asset `asset_id`. `inspectors`
    /// draws values with the editor's type inspectors, as the components of
    /// an object are drawn.
    fn show_inspector(
        &self,
        ui: &mut Ui,
        game: &mut GameContext,
        inspectors: &InspectorRegistry,
        asset_id: Uuid,
    ) {
        let _ = (ui, game, inspectors, asset_id);
    }
    fn has_context_menu(&self) -> bool {
        false
    }
    fn show_context_menu(
        &self,
        ui: &mut Ui,
        game: &mut GameContext,
        asset_id: Uuid,
    ) -> AssetInspectorAction {
        let _ = (ui, game, asset_id);
        AssetInspectorAction::None
    }
}
