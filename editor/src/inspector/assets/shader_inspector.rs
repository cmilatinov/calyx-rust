use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use engine::assets::material::Material;
use engine::context::GameContext;
use engine::{
    egui::Ui,
    reflect::{Reflect, ReflectDefault},
    render::Shader,
    utils::TypeUuid,
    uuid::Uuid,
};
use std::io::BufWriter;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct ShaderInspector;

impl AssetInspector for ShaderInspector {
    fn target_type_uuid(&self) -> Uuid {
        Shader::type_uuid()
    }

    fn has_context_menu(&self) -> bool {
        true
    }

    fn show_context_menu(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        if ui.button("Create Material").clicked() {
            'cleanup: {
                let Ok(asset) = game.assets.asset_registry.read().load_dyn_by_id(asset_id) else {
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
                game.background.thread_pool().execute(move || {
                    let material = Material::from_shader(&assets, shader);
                    let writer = BufWriter::new(file);
                    let _ = engine::serde_json::to_writer_pretty(writer, &material);
                });
            }
            ui.close_menu();
        }
    }
}
