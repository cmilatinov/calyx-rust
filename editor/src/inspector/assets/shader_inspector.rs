use std::io::BufWriter;

use engine::background::Background;
use engine::{
    assets::{material::Material, Asset},
    core::Ref,
    egui::Ui,
    reflect::{Reflect, ReflectDefault},
    render::Shader,
    utils::TypeUuid,
    uuid::Uuid,
};

use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct ShaderInspector;

impl AssetInspector for ShaderInspector {
    fn target_type_uuid(&self) -> Uuid {
        Shader::type_uuid()
    }

    fn show_context_menu(&self, ui: &mut Ui, asset: Ref<dyn Asset>) {
        if ui.button("Create Material").clicked() {
            if let Some(shader) = asset.try_downcast::<Shader>() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("material.cxmat")
                    .add_filter("cxmat", &["cxmat"])
                    .save_file()
                {
                    if let Ok(file) = std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path)
                    {
                        Background::get().thread_pool().execute(move || {
                            let material = Material::from_shader(shader);
                            let writer = BufWriter::new(file);
                            let _ = engine::serde_json::to_writer_pretty(writer, &material);
                        });
                    }
                }
            }
            ui.close_menu();
        }
    }
}
