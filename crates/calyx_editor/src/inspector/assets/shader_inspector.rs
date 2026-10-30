use crate::asset_open::{AssetOpenContext, AssetOpenOutcome, AssetOpener, ReflectAssetOpener};
use crate::inspector::asset_inspector::{
    AssetInspector, AssetInspectorAction, ReflectAssetInspector,
};
use crate::task_id::TaskId;
use calyx_engine::assets::material::Material;
use calyx_engine::context::GameContext;
use calyx_engine::{
    reflect::{Reflect, ReflectDefault},
    render::Shader,
    utils::TypeUuid,
};
use egui::Ui;
use std::io::BufWriter;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector, AssetOpener)]
pub struct ShaderInspector;

/// A shader is WGSL text, so it opens in the OS default app for its file.
impl AssetOpener for ShaderInspector {
    fn target_type_uuid(&self) -> Uuid {
        Shader::type_uuid()
    }

    fn open(&self, editor: &mut AssetOpenContext<'_>, asset_id: Uuid) -> AssetOpenOutcome {
        let path = editor
            .game
            .assets
            .registries
            .assets
            .read()
            .asset_meta_from_id(asset_id)
            .and_then(|meta| meta.path);
        let Some(path) = path else {
            let reason = format!("shader {asset_id} has no file to open");
            log::error!("Failed to open {reason}");
            return AssetOpenOutcome::Refused(reason);
        };
        match open::that_detached(&path) {
            Ok(()) => {
                log::info!("Opened shader {} in the default app", path.display());
                AssetOpenOutcome::Opened
            }
            Err(error) => {
                let reason = format!("shader {} in the default app: {error}", path.display());
                log::error!("Failed to open {reason}");
                AssetOpenOutcome::Refused(reason)
            }
        }
    }
}

impl AssetInspector for ShaderInspector {
    fn target_type_uuid(&self) -> Uuid {
        Shader::type_uuid()
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
        if ui.button("Create Material").clicked() {
            'cleanup: {
                let Ok(asset) = game
                    .assets
                    .registries
                    .assets
                    .read()
                    .load_dyn_by_id(asset_id)
                else {
                    break 'cleanup;
                };
                let Some(shader) = asset.try_downcast::<Shader>() else {
                    break 'cleanup;
                };
                let Some(path) = rfd::FileDialog::new()
                    .set_file_name("material.cxmat")
                    .add_filter("cxmat", &["cxmat"])
                    .save_file()
                else {
                    break 'cleanup;
                };
                let Ok(file) = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path)
                else {
                    break 'cleanup;
                };
                let assets = game.assets.lock_read();
                game.resources
                    .background()
                    .write()
                    .execute(TaskId::CreateMaterial, move || {
                        let material = Material::from_shader(&assets, shader);
                        let writer = BufWriter::new(file);
                        let _ = serde_json::to_writer_pretty(writer, &material);
                    });
            }
            ui.close_menu();
        }
        AssetInspectorAction::None
    }
}
