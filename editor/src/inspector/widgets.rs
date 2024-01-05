use engine::assets::{Asset, AssetRegistry};
use engine::core::OptionRef;
use engine::egui;
use engine::egui::{DragValue, Id, Ui};
use engine::glm::{Vec2, Vec3, Vec4};
use engine::uuid::Uuid;
use lazy_static::lazy_static;
use std::sync::RwLock;

pub struct Widgets;

struct AssetSelectState {
    search: String,
    should_request_focus: bool,
}

impl Widgets {
    pub fn asset_select(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        value: &mut OptionRef<dyn Asset>,
    ) -> bool {
        lazy_static! {
            static ref STATE: RwLock<AssetSelectState> = RwLock::new(AssetSelectState {
                search: String::from(""),
                should_request_focus: false
            });
        }
        let mut state = STATE.write().unwrap();
        let mut registry = AssetRegistry::get_mut();
        let mut asset_id = value
            .as_ref()
            .and_then(|r| registry.asset_id_from_ref(r))
            .unwrap_or_default();
        let asset_meta = registry.asset_meta_from_id(asset_id);
        let mut changed = false;

        let res = egui::ComboBox::from_id_source(id)
            .wrap(false)
            .width(ui.available_width())
            .selected_text(
                asset_meta
                    .map(|meta| meta.display_name.as_str())
                    .unwrap_or("None"),
            )
            .show_ui(ui, |ui| {
                if state.should_request_focus {
                    ui.memory_mut(|m| m.request_focus(Id::from("asset_select")));
                    state.should_request_focus = false;
                }
                egui::TextEdit::singleline(&mut state.search)
                    .id(Id::from("asset_select"))
                    .hint_text("Filter by name")
                    .show(ui);
                let mut assets = Vec::new();
                registry.search_assets(state.search.as_str(), type_uuid, &mut assets);
                ui.add_space(6.0);
                changed |= ui
                    .selectable_value(&mut asset_id, Uuid::nil(), "None")
                    .changed();
                for asset in assets {
                    changed |= ui
                        .selectable_value(&mut asset_id, asset.id, asset.display_name)
                        .changed();
                }
            })
            .response;
        if res.clicked() {
            state.search.clear();
            state.should_request_focus = true;
        }
        *value = registry.load_dyn_by_id(asset_id).ok().into();
        changed
    }
    pub fn drag_float4(ui: &mut Ui, speed: f32, value: &mut Vec4) -> bool {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..4])
    }

    pub fn drag_float3(ui: &mut Ui, speed: f32, value: &mut Vec3) -> bool {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..3])
    }

    pub fn drag_float2(ui: &mut Ui, speed: f32, value: &mut Vec2) -> bool {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..2])
    }

    pub fn drag_floatn(ui: &mut Ui, speed: f32, value: &mut [f32]) -> bool {
        let mut changed = false;
        ui.horizontal_centered(|ui| {
            for value in value.iter_mut() {
                changed |= ui.add(DragValue::new(value).speed(speed)).changed();
            }
        });
        changed
    }

    pub fn drag_angle(ui: &mut Ui, value: &mut f32) -> bool {
        let mut degrees = value.to_degrees();
        let res = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°"));
        let changed = res.changed();
        if changed {
            *value = degrees.to_radians();
        }
        changed
    }

    pub fn drag_angle3(ui: &mut Ui, value: &mut Vec3) -> bool {
        let mut changed = false;
        changed |= Self::drag_angle(ui, &mut value.x);
        changed |= Self::drag_angle(ui, &mut value.y);
        changed |= Self::drag_angle(ui, &mut value.z);
        changed
    }
}
