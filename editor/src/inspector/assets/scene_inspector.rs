use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Scene;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
use crate::project_manager::ProjectAssemblyStatus;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
#[repr(C)]
pub struct SceneInspector;

impl AssetInspector for SceneInspector {
    fn target_type_uuid(&self) -> Uuid {
        Scene::type_uuid()
    }

    fn has_context_menu(&self) -> bool {
        true
    }

    fn show_context_menu(
        &self,
        ui: &mut Ui,
        game: &mut GameContext,
        asset_id: Uuid,
    ) -> AssetInspectorAction {
        let assemblies_loaded = game
            .resources
            .resource::<engine::core::Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded());
        let open_response = ui
            .add_enabled(assemblies_loaded, egui::Button::new("Open"))
            .on_disabled_hover_text("Build project assemblies before opening scenes");
        if open_response.clicked() {
            ui.close_menu();
            return AssetInspectorAction::OpenScene(asset_id);
        }
        AssetInspectorAction::None
    }
}
