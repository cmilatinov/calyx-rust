use engine::assets::{Asset, AssetRef, AssetRegistry};
use engine::core::OptionRef;
use engine::egui;
use engine::egui::{DragValue, Id, Ui};
use engine::glm::{Vec2, Vec3, Vec4};
use engine::uuid::Uuid;
use lazy_static::lazy_static;
use std::any::TypeId;
use std::ops::DerefMut;
use std::sync::RwLock;

pub struct Widgets;

const DRAG_SIZE: f32 = 56.0;

impl Widgets {
    pub fn asset_select<A: Asset>(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        type_id: Option<TypeId>,
        value: &mut OptionRef<A>,
    ) -> bool {
        lazy_static! {
            static ref SEARCH: RwLock<String> = RwLock::new(String::from(""));
        }
        let mut search = SEARCH.write().unwrap();
        let mut registry = AssetRegistry::get_mut();
        let mut asset_id = value
            .as_ref()
            .and_then(|r| registry.asset_id_from_ref(&r.as_asset()))
            .unwrap_or(Uuid::nil());
        let asset_meta = registry.asset_meta_from_id(asset_id);

        egui::ComboBox::from_id_source(id)
            .wrap(false)
            .selected_text(
                asset_meta
                    .map(|meta| meta.display_name.as_str())
                    .unwrap_or("None"),
            )
            .show_ui(ui, |ui| {
                ui.memory_mut(|m| m.request_focus(Id::from("asset_select")));
                egui::TextEdit::singleline(search.deref_mut())
                    .id(Id::from("asset_select"))
                    .hint_text("Filter by name")
                    .show(ui)
                    .response
                    .changed();
                let mut assets = Vec::new();
                registry.search_assets(search.as_str(), type_id, &mut assets);
                if ui
                    .selectable_value(&mut asset_id, Uuid::nil(), "None")
                    .clicked()
                {
                    println!("{}", asset_id);
                }
                for asset in assets {
                    if ui
                        .selectable_value(&mut asset_id, asset.id, asset.display_name)
                        .clicked()
                    {
                        println!("{}", asset_id);
                    }
                }
            });
        if let Ok(asset_ref) = registry.load_by_id::<A>(asset_id) {
            *value = Some(asset_ref);
            true
        } else {
            *value = None;
            false
        }
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
                changed |= ui
                    .add_sized(
                        [DRAG_SIZE, ui.available_height()],
                        DragValue::new(value).speed(speed),
                    )
                    .changed();
            }
        });
        changed
    }

    pub fn drag_angle(ui: &mut Ui, value: &mut f32) -> bool {
        let mut degrees = value.to_degrees();
        let res = ui.add_sized(
            [DRAG_SIZE, ui.available_height()],
            DragValue::new(&mut degrees).speed(1.0).suffix("°"),
        );
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
