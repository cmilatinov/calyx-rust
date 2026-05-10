use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::widgets::Widgets;
use egui;
use egui::Ui;
use engine::assets::material::{Material, MaterialTexture, ShaderVariable, ShaderVariableValue};
use engine::assets::texture::Texture;
use engine::context::{AssetContext, GameContext};
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use serde_json;
use std::io::BufWriter;
use std::ops::Deref;
use std::time::Duration;
use uuid::Uuid;

const MATERIAL_COLOR_COMMIT_DELAY: f64 = 0.15;

#[derive(Clone, Copy)]
struct MaterialColorEditState {
    draft: [f32; 4],
    dirty: bool,
    last_changed: f64,
}

impl MaterialColorEditState {
    fn clean(color: [f32; 4]) -> Self {
        Self {
            draft: color,
            dirty: false,
            last_changed: 0.0,
        }
    }
}

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, AssetInspector)]
#[repr(C)]
pub struct MaterialInspector;

impl AssetInspector for MaterialInspector {
    fn target_type_uuid(&self) -> Uuid {
        Material::type_uuid()
    }
    fn show_inspector(&self, ui: &mut Ui, game: &mut GameContext, asset_id: Uuid) {
        let Ok(asset) = game
            .assets
            .registries
            .assets
            .read()
            .load_dyn_by_id(asset_id)
        else {
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
            Self::flush_pending_color_edits(ui, &mut material);
            let Some(meta) = game
                .assets
                .registries
                .assets
                .read()
                .asset_meta_from_ref_dyn(&asset.readonly())
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
    fn color_edit_id(var: &ShaderVariable) -> egui::Id {
        egui::Id::new((
            "material_texture_color",
            var.group,
            var.binding,
            var.offset,
            var.name.as_str(),
        ))
    }

    fn flush_pending_color_edits(ui: &mut Ui, material: &mut Material) {
        for var in material.variables.iter_mut() {
            let id = Self::color_edit_id(var);
            let ShaderVariableValue::Texture2D(MaterialTexture::Color(ref mut color)) =
                &mut var.value
            else {
                continue;
            };
            let mut state = ui
                .memory_mut(|mem| mem.data.get_temp::<MaterialColorEditState>(id))
                .unwrap_or_else(|| MaterialColorEditState::clean(*color));
            if state.dirty {
                *color = state.draft;
                state.dirty = false;
                ui.memory_mut(|mem| mem.data.insert_temp(id, state));
            }
        }
    }

    fn show_variable_inspector(ui: &mut Ui, game: &AssetContext, var: &mut ShaderVariable) {
        let show_var = match &var.value {
            ShaderVariableValue::Sampler => false,
            _ => true,
        };
        if !show_var {
            return;
        }
        let color_edit_id = Self::color_edit_id(var);
        Widgets::inspector_prop_value(ui, var.name.as_str(), |ui, _| match &mut var.value {
            ShaderVariableValue::Bool(ref mut bool) => {
                ui.checkbox(bool, "");
            }
            ShaderVariableValue::Color(ref mut color) => {
                ui.color_edit_button_rgba_unmultiplied(color);
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
            ShaderVariableValue::Texture2D(ref mut texture) => {
                let id = (var.group, var.binding, var.offset, "texture_source");
                egui::ComboBox::from_id_salt(id)
                    .selected_text(match texture {
                        MaterialTexture::Asset(_) => "Texture",
                        MaterialTexture::Color(_) => "Color",
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(matches!(texture, MaterialTexture::Color(_)), "Color")
                            .clicked()
                        {
                            *texture = MaterialTexture::Color([1.0, 1.0, 1.0, 1.0]);
                            ui.memory_mut(|mem| {
                                mem.data.insert_temp(
                                    color_edit_id,
                                    MaterialColorEditState::clean([1.0, 1.0, 1.0, 1.0]),
                                )
                            });
                        }
                        if ui
                            .selectable_label(
                                matches!(texture, MaterialTexture::Asset(_)),
                                "Texture",
                            )
                            .clicked()
                        {
                            *texture = MaterialTexture::Asset(Default::default());
                        }
                    });
                match texture {
                    MaterialTexture::Asset(ref mut tex) => {
                        Widgets::asset_select_t(
                            ui,
                            &game.registries.assets.read(),
                            (var.group, var.binding, var.offset),
                            Some(Texture::type_uuid()),
                            tex,
                        );
                    }
                    MaterialTexture::Color(ref mut color) => {
                        Self::show_material_color_editor(ui, color_edit_id, color);
                    }
                }
            }
            _ => {}
        });
    }

    fn show_material_color_editor(ui: &mut Ui, id: egui::Id, color: &mut [f32; 4]) {
        let (now, pointer_down) = ui.input(|input| (input.time, input.pointer.any_down()));
        let mut state = ui
            .memory_mut(|mem| mem.data.get_temp::<MaterialColorEditState>(id))
            .unwrap_or_else(|| MaterialColorEditState::clean(*color));
        if !state.dirty {
            state.draft = *color;
        }

        let response = ui.color_edit_button_rgba_unmultiplied(&mut state.draft);
        if response.changed() {
            state.dirty = true;
            state.last_changed = now;
        }

        if state.dirty {
            let elapsed = now - state.last_changed;
            if !pointer_down || elapsed >= MATERIAL_COLOR_COMMIT_DELAY {
                *color = state.draft;
                state.dirty = false;
            } else {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(
                    MATERIAL_COLOR_COMMIT_DELAY - elapsed,
                ));
            }
        }

        ui.memory_mut(|mem| mem.data.insert_temp(id, state));
    }
}
