use crate::inspector::asset_inspector::{AssetInspector, ReflectAssetInspector};
use crate::inspector::widgets::{AssetSelectorFilter, Widgets};
use egui;
use egui::Ui;
use engine::assets::material::{Material, MaterialTexture, ShaderVariable, ShaderVariableValue};
use engine::assets::texture::Texture;
use engine::assets::Asset;
use engine::context::{AssetContext, GameContext};
use engine::core::Ref;
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use serde_json;
use std::io::BufWriter;
use std::ops::Deref;
use std::time::Duration;
use uuid::Uuid;

const MATERIAL_AUTOSAVE_DELAY: f64 = 1.0;

#[derive(Clone, Copy)]
struct MaterialColorEditState {
    draft: [f32; 4],
    dirty: bool,
}

impl MaterialColorEditState {
    fn clean(color: [f32; 4]) -> Self {
        Self {
            draft: color,
            dirty: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct MaterialAutosaveState {
    dirty: bool,
    last_changed: f64,
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
        let mut changed = false;
        for var in material.variables.iter_mut() {
            changed |= Self::show_variable_inspector(ui, &game.assets, var);
        }
        if changed {
            Self::mark_material_dirty(ui, asset_id);
        }
        Self::autosave_material(ui, &game.assets, &asset, asset_id, material.deref());
    }
}

impl MaterialInspector {
    fn autosave_id(asset_id: Uuid) -> egui::Id {
        egui::Id::new(("material_autosave", asset_id))
    }

    fn color_edit_id(var: &ShaderVariable) -> egui::Id {
        egui::Id::new((
            "material_texture_color",
            var.group,
            var.binding,
            var.offset,
            var.name.as_str(),
        ))
    }

    fn mark_material_dirty(ui: &mut Ui, asset_id: Uuid) {
        let now = ui.input(|input| input.time);
        ui.memory_mut(|mem| {
            mem.data.insert_temp(
                Self::autosave_id(asset_id),
                MaterialAutosaveState {
                    dirty: true,
                    last_changed: now,
                },
            );
        });
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(MATERIAL_AUTOSAVE_DELAY));
    }

    fn autosave_material(
        ui: &mut Ui,
        assets: &AssetContext,
        asset: &Ref<dyn Asset>,
        asset_id: Uuid,
        material: &Material,
    ) {
        let id = Self::autosave_id(asset_id);
        let Some(mut state) = ui.memory_mut(|mem| mem.data.get_temp::<MaterialAutosaveState>(id))
        else {
            return;
        };
        if !state.dirty {
            return;
        }

        let now = ui.input(|input| input.time);
        let elapsed = now - state.last_changed;
        if elapsed < MATERIAL_AUTOSAVE_DELAY {
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(MATERIAL_AUTOSAVE_DELAY - elapsed));
            return;
        }

        Self::write_material_to_disk(assets, asset, material);
        state.dirty = false;
        ui.memory_mut(|mem| mem.data.insert_temp(id, state));
    }

    fn write_material_to_disk(assets: &AssetContext, asset: &Ref<dyn Asset>, material: &Material) {
        let Some(meta) = assets
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
        let _ = serde_json::to_writer_pretty(writer, material);
    }

    fn show_variable_inspector(ui: &mut Ui, game: &AssetContext, var: &mut ShaderVariable) -> bool {
        let show_var = !matches!(&var.value, ShaderVariableValue::Sampler);
        if !show_var {
            return false;
        }
        let color_edit_id = Self::color_edit_id(var);
        let mut changed = false;
        Widgets::inspector_prop_value(ui, var.name.as_str(), |ui, _| match &mut var.value {
            ShaderVariableValue::Bool(ref mut bool) => {
                changed |= ui.checkbox(bool, "").changed();
            }
            ShaderVariableValue::Color(ref mut color) => {
                changed |= ui.color_edit_button_rgba_unmultiplied(color).changed();
            }
            ShaderVariableValue::Int(ref mut int) => {
                changed |= ui.add(egui::DragValue::new(int)).changed();
            }
            ShaderVariableValue::Uint(ref mut uint) => {
                changed |= ui.add(egui::DragValue::new(uint)).changed();
            }
            ShaderVariableValue::Float(ref mut float) => {
                changed |= ui.add(egui::DragValue::new(float).speed(0.1)).changed();
            }
            ShaderVariableValue::Vec2(ref mut vec) => {
                changed |= Widgets::drag_floatn(ui, 0.1, vec);
            }
            ShaderVariableValue::Vec3(ref mut vec) => {
                changed |= Widgets::drag_floatn(ui, 0.1, vec);
            }
            ShaderVariableValue::Vec4(ref mut vec) => {
                changed |= Widgets::drag_floatn(ui, 0.1, vec);
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
                            changed = true;
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
                            changed = true;
                        }
                    });
                match texture {
                    MaterialTexture::Asset(ref mut tex) => {
                        changed |= Widgets::asset_select_t_filtered(
                            ui,
                            &game.registries.assets.read(),
                            (var.group, var.binding, var.offset),
                            Some(Texture::type_uuid()),
                            &[AssetSelectorFilter::Texture2D],
                            tex,
                        )
                        .changed();
                    }
                    MaterialTexture::Color(ref mut color) => {
                        changed |= Self::show_material_color_editor(ui, color_edit_id, color);
                    }
                }
            }
            _ => {}
        });
        changed
    }

    fn show_material_color_editor(ui: &mut Ui, id: egui::Id, color: &mut [f32; 4]) -> bool {
        let pointer_down = ui.input(|input| input.pointer.any_down());
        let mut state = ui
            .memory_mut(|mem| mem.data.get_temp::<MaterialColorEditState>(id))
            .unwrap_or_else(|| MaterialColorEditState::clean(*color));
        if !state.dirty {
            state.draft = *color;
        }

        let response = ui.color_edit_button_rgba_unmultiplied(&mut state.draft);
        if response.changed() {
            state.dirty = true;
        }

        let mut committed = false;
        if state.dirty {
            if !pointer_down {
                *color = state.draft;
                state.dirty = false;
                committed = true;
            } else {
                ui.ctx().request_repaint();
            }
        }

        ui.memory_mut(|mem| mem.data.insert_temp(id, state));
        committed
    }
}
