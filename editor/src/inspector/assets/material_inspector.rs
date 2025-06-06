use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::widgets::Widgets;
use egui;
use egui::Ui;
use engine::assets::material::{Material, ShaderVariable, ShaderVariableValue};
use engine::assets::texture::Texture;
use engine::context::{AssetContext, GameContext};
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use serde_json;
use std::io::BufWriter;
use std::ops::Deref;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
pub struct MaterialInspector;

impl AssetInspector for MaterialInspector {
    fn target_type_uuid(&self) -> Uuid {
        Material::type_uuid()
    }
    fn show_inspector(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let Ok(asset) = game.assets.asset_registry.read().load_dyn_by_id(asset_id) else {
            return;
        };
        let Some(material_ref) = asset.try_downcast::<Material>() else {
            return;
        };
        let mut material = material_ref.write();
        for var in material.variables.iter_mut() {
            Self::show_variable_inspector(ui, &game.assets, var);
        }
        if ui.button("Save").clicked() {
            let Some(meta) = game
                .assets
                .asset_registry
                .read()
                .asset_meta_from_ref(&asset)
            else {
                return;
            };
            let Some(path) = meta.path.as_ref() else {
                return;
            };
            let Ok(file) = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(path)
            else {
                return;
            };
            let writer = BufWriter::new(file);
            let _ = serde_json::to_writer_pretty(writer, material.deref());
        }
    }
}

impl MaterialInspector {
    fn show_variable_inspector(ui: &mut Ui, game: &AssetContext, var: &mut ShaderVariable) {
        let show_var = match &var.value {
            ShaderVariableValue::Sampler => false,
            _ => true,
        };
        if !show_var {
            return;
        }
        Widgets::inspector_prop_value(ui, var.name.as_str(), |ui, _| match &mut var.value {
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
                    &game.asset_registry.read(),
                    (var.group, var.binding, var.offset),
                    Some(Texture::type_uuid()),
                    tex,
                );
            }
            _ => {}
        });
    }
}
