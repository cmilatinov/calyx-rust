use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::widgets::Widgets;
use crate::BASE_FONT_SIZE;
use engine::assets::material::{Material, ShaderVariable, ShaderVariableValue};
use engine::assets::texture::Texture;
use engine::assets::{Asset, AssetRegistry};
use engine::core::Ref;
use engine::egui::Ui;
use engine::egui_extras::{Column, TableBody};
use engine::reflect::{Reflect, ReflectDefault};
use engine::serde_json;
use engine::utils::TypeUuid;
use engine::uuid::Uuid;
use engine::{egui, egui_extras};
use std::io::BufWriter;
use std::ops::Deref;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct MaterialInspector;

impl AssetInspector for MaterialInspector {
    fn target_type_uuid(&self) -> Uuid {
        Material::type_uuid()
    }
    fn show_inspector(&self, ui: &mut Ui, asset: Ref<dyn Asset>) {
        if let Some(material_ref) = asset.try_downcast::<Material>() {
            let mut material = material_ref.write();
            egui_extras::TableBuilder::new(ui)
                .column(
                    Column::auto_with_initial_suggestion(200.0)
                        .clip(true)
                        .resizable(true),
                )
                .column(Column::remainder().clip(true))
                .body(|mut body| {
                    for var in material.variables.iter_mut() {
                        Self::show_variable_inspector(&mut body, var);
                    }
                });
            if ui
                .add_sized(
                    [ui.available_width(), BASE_FONT_SIZE + 4.0],
                    egui::Button::new("Save"),
                )
                .clicked()
            {
                if let Some(meta) = AssetRegistry::get().asset_meta_from_ref(&asset) {
                    if let Some(path) = meta.path.as_ref() {
                        if let Ok(file) = std::fs::OpenOptions::new()
                            .write(true)
                            .truncate(true)
                            .open(path)
                        {
                            let writer = BufWriter::new(file);
                            let _ = serde_json::to_writer_pretty(writer, material.deref());
                        }
                    }
                }
            }
        }
    }
}

impl MaterialInspector {
    fn show_variable_inspector(body: &mut TableBody, var: &mut ShaderVariable) {
        let show_var = match &var.value {
            ShaderVariableValue::Sampler => false,
            _ => true,
        };
        if !show_var {
            return;
        }
        body.row(BASE_FONT_SIZE + 6.0, |mut row| {
            row.col(|ui| {
                ui.add(egui::Label::new(var.name.as_str()).wrap(false));
            });
            row.col(|ui| match &mut var.value {
                ShaderVariableValue::Bool(ref mut bool) => {
                    ui.checkbox(bool, "");
                }
                ShaderVariableValue::Color(ref mut color) => {
                    ui.color_edit_button_srgba(color);
                }
                ShaderVariableValue::Int(ref mut int) => {
                    ui.add(egui::DragValue::new(int));
                }
                ShaderVariableValue::Uint(ref mut uint) => {
                    ui.add(egui::DragValue::new(uint));
                }
                ShaderVariableValue::Float(ref mut float) => {
                    ui.add(egui::DragValue::new(float).speed(0.1));
                }
                ShaderVariableValue::Vec2(ref mut vec) => {
                    Widgets::drag_floatn(ui, 0.1, vec);
                }
                ShaderVariableValue::Vec3(ref mut vec) => {
                    Widgets::drag_floatn(ui, 0.1, vec);
                }
                ShaderVariableValue::Vec4(ref mut vec) => {
                    Widgets::drag_floatn(ui, 0.1, vec);
                }
                ShaderVariableValue::Texture2D(ref mut tex) => {
                    Widgets::asset_select_t(
                        ui,
                        (var.group, var.binding, var.offset),
                        Some(Texture::type_uuid()),
                        tex,
                    );
                }
                _ => {}
            });
        });
    }
}
