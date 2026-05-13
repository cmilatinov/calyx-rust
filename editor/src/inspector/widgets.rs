use egui;
use egui::{Color32, DragValue, Frame, Id, Response, Shape, Stroke, StrokeKind, Ui, WidgetText};
use engine::assets::texture::Texture;
use engine::assets::{Asset, AssetAccess, AssetMeta, AssetRef, AssetRegistry};
use engine::component::ComponentID;
use engine::scene::{GameObjectRef, Scene};
use engine::utils::TypeUuid;
use lazy_static::lazy_static;
use nalgebra_glm::{Vec2, Vec3, Vec4};
use re_ui::list_item::{ListItemContent, ListVisuals};
use re_ui::UiExt;
use std::cell::RefCell;
use std::sync::RwLock;
use uuid::Uuid;

pub struct Widgets;

pub enum PropChildrenPhase<'a> {
    Value {
        ui: &'a mut Ui,
        #[allow(unused)]
        visuals: ListVisuals,
    },
    Children {
        ui: &'a mut Ui,
    },
}

#[derive(Default)]
pub(crate) struct SearchSelectState {
    search: String,
    should_request_focus: bool,
}

impl SearchSelectState {
    pub(crate) fn open(&mut self) {
        self.search.clear();
        self.should_request_focus = true;
    }

    pub(crate) fn clear_search(&mut self) {
        self.search.clear();
        self.should_request_focus = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetSelectorFilter {
    Texture2D,
}

#[allow(dead_code)]
impl Widgets {
    pub fn asset_select_t<T: Asset + TypeUuid>(
        ui: &mut Ui,
        registry: &AssetRegistry,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        value: &mut AssetRef<T>,
    ) -> Response {
        let res = Self::asset_select(ui, registry, id, type_uuid, value.id_mut());
        if res.changed() {
            value.clear_cache();
        }
        res
    }

    pub fn asset_select_t_filtered<T: Asset + TypeUuid>(
        ui: &mut Ui,
        registry: &AssetRegistry,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        filters: &[AssetSelectorFilter],
        value: &mut AssetRef<T>,
    ) -> Response {
        let res = Self::asset_select_filtered(ui, registry, id, type_uuid, filters, value.id_mut());
        if res.changed() {
            value.clear_cache();
        }
        res
    }

    pub fn asset_select(
        ui: &mut Ui,
        registry: &AssetRegistry,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        value: &mut Uuid,
    ) -> Response {
        Self::asset_select_filtered(ui, registry, id, type_uuid, &[], value)
    }

    pub fn asset_select_filtered(
        ui: &mut Ui,
        registry: &AssetRegistry,
        id: impl std::hash::Hash,
        type_uuid: Option<Uuid>,
        filters: &[AssetSelectorFilter],
        value: &mut Uuid,
    ) -> Response {
        lazy_static! {
            static ref STATE: RwLock<SearchSelectState> = RwLock::new(SearchSelectState::default());
        }
        let mut state = STATE.write().unwrap();
        let asset_meta = registry.asset_meta_from_id(*value);
        let mut changed = false;

        let id = ui.make_persistent_id(id);
        let selected_text = asset_meta
            .map(|meta| meta.display_name.clone())
            .unwrap_or("None".into());
        let mut res = egui::ComboBox::from_id_salt(id)
            .truncate()
            .width(100.0)
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                Self::search_select_contents(ui, id, &mut state, |ui, search| {
                    let mut assets = Vec::new();
                    registry.search_assets(search, type_uuid, &mut assets);
                    assets.retain(|asset| {
                        filters
                            .iter()
                            .all(|filter| Self::asset_matches_filter(registry, asset, *filter))
                    });
                    if search.is_empty() {
                        changed |= ui.selectable_value(value, Uuid::nil(), "None").changed();
                    }
                    for asset in assets {
                        changed |= ui
                            .selectable_value(value, asset.id, asset.display_name)
                            .changed();
                    }
                });
            })
            .response;
        if res.clicked() {
            state.open();
        }
        if changed {
            res.mark_changed();
        }
        res
    }

    pub(crate) fn search_select_contents(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        state: &mut SearchSelectState,
        add_options: impl FnOnce(&mut Ui, &str),
    ) {
        let id = ui.make_persistent_id(id);
        let search_id = id.with("search");
        if state.should_request_focus {
            ui.memory_mut(|m| m.request_focus(search_id));
            state.should_request_focus = false;
        }
        ui.add_space(1.0);
        egui::TextEdit::singleline(&mut state.search)
            .id(search_id)
            .hint_text("Filter by name")
            .show(ui);
        ui.add_space(6.0);
        add_options(ui, state.search.as_str());
    }

    fn asset_matches_filter(
        registry: &AssetRegistry,
        asset: &AssetMeta,
        filter: AssetSelectorFilter,
    ) -> bool {
        match filter {
            AssetSelectorFilter::Texture2D => Self::asset_is_texture_2d(registry, asset),
        }
    }

    fn asset_is_texture_2d(registry: &AssetRegistry, asset: &AssetMeta) -> bool {
        if asset.type_uuid != Texture::type_uuid() {
            return false;
        }
        if asset.path.is_some() {
            return true;
        }
        registry
            .loaded_by_id::<Texture>(asset.id)
            .map(|texture| texture.read().is_2d())
            .unwrap_or(false)
    }

    pub fn game_object_select(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        scene: &Scene,
        value: &mut GameObjectRef,
    ) -> Response {
        lazy_static! {
            static ref STATE: RwLock<SearchSelectState> = RwLock::new(SearchSelectState {
                search: String::from(""),
                should_request_focus: false
            });
        }
        let mut state = STATE.write().unwrap();
        let mut changed = false;
        let mut game_object_id = value.id();
        let (mut res, dropped_payload) = ui
            .scope(|ui| {
                let visuals = ui.visuals_mut();
                visuals.widgets.inactive.bg_fill = visuals.panel_fill;
                ui.dnd_drop_zone::<Uuid, Response>(Frame::NONE, |ui| {
                    egui::ComboBox::from_id_salt(id)
                        .truncate()
                        .width(100.0)
                        .selected_text(
                            value
                                .game_object(scene)
                                .map(|go| scene.name(go))
                                .unwrap_or(String::from("None")),
                        )
                        .show_ui(ui, |ui| {
                            let id = Id::from("game_object_select");
                            if state.should_request_focus {
                                ui.memory_mut(|m| m.request_focus(id));
                                state.should_request_focus = false;
                            }
                            egui::TextEdit::singleline(&mut state.search)
                                .id(id)
                                .hint_text("Filter by name")
                                .show(ui);
                            if state.search.is_empty() {
                                changed |= ui
                                    .selectable_value(&mut game_object_id, Uuid::nil(), "None")
                                    .changed();
                            }
                            for go in scene.objects() {
                                let Some(entry) = scene.entry(go) else {
                                    return;
                                };
                                let id = scene.uuid(go);
                                let name = entry
                                    .get_component::<ComponentID>()
                                    .map(|c| c.name.as_str())
                                    .unwrap_or("");
                                changed |=
                                    ui.selectable_value(&mut game_object_id, id, name).changed();
                            }
                        })
                        .response
                })
            })
            .inner;
        if egui::DragAndDrop::has_payload_of_type::<Uuid>(ui.ctx())
            && ui.rect_contains_pointer(res.inner.rect)
        {
            let stroke_width = 1.0;
            ui.painter().add(Shape::rect_stroke(
                res.inner.rect.expand(stroke_width),
                ui.visuals().widgets.hovered.corner_radius,
                Stroke::new(stroke_width, Color32::YELLOW),
                StrokeKind::Middle,
            ));
        }
        if let Some(dropped_payload) = dropped_payload {
            game_object_id = *dropped_payload;
        }
        if res.inner.clicked() {
            state.search.clear();
            state.should_request_focus = true;
        }
        if changed {
            res.inner.mark_changed();
        }
        *value = GameObjectRef::new(game_object_id);
        res.inner
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

    pub fn inspector_prop_value<F: FnOnce(&mut Ui, ListVisuals)>(
        ui: &mut Ui,
        text: impl Into<WidgetText>,
        add_value_contents: F,
    ) {
        ui.list_item_flat_noninteractive(
            re_ui::list_item::PropertyContent::new(text).value_fn(add_value_contents),
        );
    }

    pub fn inspector_prop_children<F: FnOnce(&mut Ui)>(
        ui: &mut Ui,
        content: impl ListItemContent,
        add_children: F,
    ) {
        re_ui::list_item::ListItem::new().show_hierarchical_with_children(
            ui,
            "id".into(),
            true,
            content,
            add_children,
        );
    }

    pub fn inspector_prop_value_children<F>(
        ui: &mut Ui,
        text: impl Into<WidgetText>,
        add_contents: F,
    ) where
        F: FnMut(PropChildrenPhase<'_>),
    {
        let add_contents = RefCell::new(add_contents);
        re_ui::list_item::ListItem::new()
            .interactive(false)
            .show_hierarchical_with_children(
                ui,
                "id".into(),
                true,
                re_ui::list_item::PropertyContent::new(text)
                    .show_only_when_collapsed(false)
                    .value_fn(|ui, visuals| {
                        add_contents.borrow_mut()(PropChildrenPhase::Value { ui, visuals });
                    }),
                |ui| {
                    add_contents.borrow_mut()(PropChildrenPhase::Children { ui });
                },
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_wgpu::wgpu;

    #[test]
    fn texture_2d_filter_accepts_loaded_2d_texture() {
        let context = engine::test_support::test_asset_context_with_assets(Vec::new());
        let render_context = context.render_context.clone();
        let texture = context
            .registries
            .assets
            .read()
            .create(
                "test_texture_2d".into(),
                Texture::solid_color_2d(render_context, "test_texture_2d", [255, 255, 255, 255]),
            )
            .unwrap();
        let registry = context.registries.assets.read();
        let meta = registry.asset_meta_from_id(texture.id()).unwrap();

        assert!(Widgets::asset_is_texture_2d(&registry, &meta));
    }

    #[test]
    fn texture_2d_filter_rejects_loaded_cube_texture() {
        let context = engine::test_support::test_asset_context_with_assets(Vec::new());
        let render_context = context.render_context.clone();
        let texture = context
            .registries
            .assets
            .read()
            .create(
                "test_texture_cube".into(),
                Texture::new(
                    render_context,
                    &wgpu::TextureDescriptor {
                        label: Some("test_texture_cube"),
                        size: wgpu::Extent3d {
                            width: 16,
                            height: 16,
                            depth_or_array_layers: 6,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    None,
                    Some(wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::Cube),
                        ..Default::default()
                    }),
                    false,
                ),
            )
            .unwrap();
        let registry = context.registries.assets.read();
        let meta = registry.asset_meta_from_id(texture.id()).unwrap();

        assert!(!Widgets::asset_is_texture_2d(&registry, &meta));
    }
}
