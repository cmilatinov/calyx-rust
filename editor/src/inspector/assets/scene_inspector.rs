use egui::Ui;
use engine::context::GameContext;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::Scene;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
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

    fn show_context_menu(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let assemblies_loaded = game
            .resources
            .resource::<engine::core::Ref<ProjectAssemblyStatus>>()
            .is_some_and(|status| status.read().is_loaded());
        let open_response = ui
            .add_enabled(assemblies_loaded, egui::Button::new("Open"))
            .on_disabled_hover_text("Build project assemblies before opening scenes");
        if open_response.clicked() {
            let (scene, label) = {
                let registry = game.assets.registries.assets.read();
                let label = registry
                    .asset_meta_from_id(asset_id)
                    .and_then(|meta| meta.path.map(|path| path.display().to_string()))
                    .unwrap_or_else(|| asset_id.to_string());
                (registry.reload_by_id::<Scene>(asset_id), label)
            };

            match scene {
                Ok(scene) => {
                    game.scenes.load_scene(scene.readonly());
                    let object_count = game.scenes.current_scene().objects().count();
                    let message = format!("Opened scene {label} ({object_count} objects)");
                    log::info!("{message}");
                }
                Err(error) => {
                    let message = format!("Failed to open scene {label}: {error}");
                    log::error!("{message}");
                }
            }
            ui.close_menu();
        }
    }
}
