use crate::BASE_FONT_SIZE;
use engine::assets::{Asset, AssetOptionRef, AssetRegistry};
use engine::core::Ref;
use engine::egui::{Align, DragValue, Id, Layout, Ui, Widget, WidgetText};
use engine::egui_extras::{Column, TableBody};
use engine::glm::{Vec2, Vec3, Vec4};
use engine::utils::TypeUuid;
use engine::uuid::Uuid;
use engine::{egui, egui_extras};
use lazy_static::lazy_static;
use std::sync::RwLock;

pub struct Widgets;

struct AssetSelectState {
    search: String,
    should_request_focus: bool,
}

impl Widgets {
    pub fn asset_select_t<T: Asset + TypeUuid>(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        value: &mut Option<Ref<T>>,
    ) -> bool {
        let mut asset_ref = value.as_asset_option();
        let changed = Self::asset_select(ui, id, type_uuid, &mut asset_ref);
        if changed {
            value.set(asset_ref);
        }
        changed
    }

    pub fn asset_select(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        value: &mut Option<Ref<dyn Asset>>,
    ) -> bool {
        lazy_static! {
            static ref STATE: RwLock<AssetSelectState> = RwLock::new(AssetSelectState {
                search: String::from(""),
                should_request_focus: false
            });
        }
        let mut state = STATE.write().unwrap();
        let registry = AssetRegistry::get();
        let mut asset_id = value
            .as_ref()
            .and_then(|r| registry.asset_id_from_ref(r))
            .unwrap_or_default();
        let asset_meta = registry.asset_meta_from_id(asset_id);
        let mut changed = false;

        let res = egui::ComboBox::from_id_source(id)
            .wrap()
            .width(ui.available_width())
            .selected_text(
                asset_meta
                    .map(|meta| meta.display_name.clone())
                    .unwrap_or("None".into()),
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

    pub fn inspector_prop_table<F: FnOnce(TableBody)>(ui: &mut Ui, add_body_contents: F) {
        egui_extras::TableBuilder::new(ui)
            .column(Column::auto().clip(true).resizable(true))
            .column(Column::remainder().clip(true))
            .cell_layout(Layout::left_to_right(Align::Center))
            .body(add_body_contents);
    }

    pub fn inspector_row<F: FnOnce(&mut Ui)>(
        body: &mut TableBody,
        text: impl Into<WidgetText>,
        add_value_contents: F,
    ) {
        Self::inspector_row_label(body, egui::Label::new(text), add_value_contents);
    }

    pub fn inspector_row_label<F: FnOnce(&mut Ui)>(
        body: &mut TableBody,
        label_widget: impl Widget,
        add_value_contents: F,
    ) {
        body.row(BASE_FONT_SIZE + 6.0, |mut row| {
            row.col(|ui| {
                ui.add(label_widget);
            });
            row.col(add_value_contents);
        });
    }
}
