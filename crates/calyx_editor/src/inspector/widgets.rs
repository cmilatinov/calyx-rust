use calyx_engine::assets::texture::Texture;
use calyx_engine::assets::{Asset, AssetAccess, AssetMeta, AssetRef, AssetRegistry};
use calyx_engine::scene::{GameObject, GameObjectRef, SceneView};
use calyx_engine::utils::TypeUuid;
use egui;
use egui::{Color32, DragValue, Frame, Id, Response, Shape, Stroke, StrokeKind, Ui, WidgetText};
use lazy_static::lazy_static;
use nalgebra_glm::{Vec2, Vec3, Vec4};
use re_ui::list_item::{ListItemContent, ListVisuals};
use re_ui::UiExt;
use std::cell::RefCell;
use std::sync::RwLock;
use uuid::Uuid;

use super::type_inspector::InspectorResponse;
use crate::widgets::{ThumbnailHandle, ThumbnailPriority, ThumbnailRequest};

/// Side of the thumbnail beside an asset selector's selected value, in points.
const ASSET_SELECT_THUMBNAIL_SIZE: f32 = 18.0;
/// Side of the thumbnail beside each entry of an open asset selector, in points.
const ASSET_SELECT_ENTRY_THUMBNAIL_SIZE: f32 = 24.0;

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
        let thumbnails = Self::asset_select_thumbnails(ui, type_uuid);

        let id = ui.make_persistent_id(id);
        let selected_text = asset_meta
            .as_ref()
            .map(|meta| meta.display_name.clone())
            .unwrap_or("None".into());
        let show_combo = |ui: &mut Ui| {
            egui::ComboBox::from_id_salt(id)
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
                            let Some(thumbnails) = thumbnails.as_ref() else {
                                changed |= ui
                                    .selectable_value(value, asset.id, asset.display_name)
                                    .changed();
                                continue;
                            };
                            ui.horizontal(|ui| {
                                Self::asset_thumbnail(
                                    ui,
                                    registry,
                                    thumbnails,
                                    Some(asset.id),
                                    ASSET_SELECT_ENTRY_THUMBNAIL_SIZE,
                                    ThumbnailPriority::High,
                                );
                                changed |= ui
                                    .selectable_value(value, asset.id, asset.display_name)
                                    .changed();
                            });
                        }
                    });
                })
                .response
        };
        let mut res = match thumbnails.as_ref() {
            Some(thumbnails) => {
                let selected_id = asset_meta.as_ref().map(|meta| meta.id);
                ui.horizontal(|ui| {
                    Self::asset_thumbnail(
                        ui,
                        registry,
                        thumbnails,
                        selected_id,
                        ASSET_SELECT_THUMBNAIL_SIZE,
                        ThumbnailPriority::Normal,
                    );
                    show_combo(ui)
                })
                .inner
            }
            None => show_combo(ui),
        };
        if res.clicked() {
            state.open();
        }
        if changed {
            res.mark_changed();
        }
        res
    }

    /// The thumbnail handle an asset selector for `type_uuid` uses, or `None`
    /// when the editor installed none or that asset type has no thumbnails.
    fn asset_select_thumbnails(ui: &Ui, type_uuid: Option<Uuid>) -> Option<ThumbnailHandle> {
        if type_uuid.is_some_and(|type_uuid| !ThumbnailRequest::is_supported_asset_type(type_uuid))
        {
            return None;
        }
        ThumbnailHandle::from_ctx(ui.ctx())
    }

    /// Allocates a `size` square and, when it is on screen, requests and draws
    /// the thumbnail of `asset_id` in it with its loading or fallback state.
    /// Space is allocated even without an asset so rows stay aligned.
    fn asset_thumbnail(
        ui: &mut Ui,
        registry: &AssetRegistry,
        thumbnails: &ThumbnailHandle,
        asset_id: Option<Uuid>,
        size: f32,
        priority: ThumbnailPriority,
    ) {
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
        let Some(asset_id) = asset_id else {
            return;
        };
        if !ui.is_rect_visible(rect) {
            return;
        }
        let view = thumbnails.view_for_asset(registry, asset_id, priority);
        view.paint(
            ui,
            rect,
            egui::include_image!("../../../../resources/icons/body_dark.png"),
        );
        if let Some(text) = view.hover_text() {
            response.on_hover_text(text);
        }
    }

    pub(crate) fn search_select_contents(
        ui: &mut Ui,
        id: impl std::hash::Hash,
        state: &mut SearchSelectState,
        add_options: impl FnOnce(&mut Ui, &str),
    ) {
        let id = ui.make_persistent_id(id);
        let search_id = id.with("search");
        ui.add_space(1.0);
        let search_response = egui::TextEdit::singleline(&mut state.search)
            .id(search_id)
            .hint_text("Filter by name")
            .show(ui);
        if state.should_request_focus {
            search_response.response.request_focus();
            state.should_request_focus = false;
        }
        ui.add_space(6.0);
        add_options(ui, state.search.as_str());
    }

    /// Applies lightweight selector-side asset constraints without forcing asset
    /// loads from the UI path. File-backed `Texture` assets currently rely on
    /// the importer invariant that image files produce single-sample 2D
    /// textures; if file-backed 3D, array, cube, or multisampled textures are
    /// introduced, this should use imported metadata instead of `path` alone.
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
        scene: &SceneView,
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
                            scene
                                .find(value.id())
                                .map(|go| Self::game_object_label(scene, go))
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
                                let id = scene.uuid(go);
                                let label = Self::game_object_label(scene, go);
                                changed |= ui
                                    .selectable_value(&mut game_object_id, id, label)
                                    .changed();
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
            changed |= game_object_id != *dropped_payload;
            game_object_id = *dropped_payload;
        }
        if res.inner.clicked() {
            state.search.clear();
            state.should_request_focus = true;
        }
        if changed {
            res.inner.mark_changed();
        }
        if game_object_id != value.id() {
            *value = GameObjectRef::new(game_object_id);
        }
        res.inner
    }

    /// An object's name, or its id when `scene` cannot read names, as while
    /// the inspector edits a [`ComponentID`](calyx_engine::component::ComponentID).
    fn game_object_label(scene: &SceneView, game_object: GameObject) -> String {
        scene
            .name(game_object)
            .unwrap_or_else(|| scene.uuid(game_object).to_string())
    }

    pub fn drag_float4(ui: &mut Ui, speed: f32, value: &mut Vec4) -> InspectorResponse {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..4])
    }

    pub fn drag_float3(ui: &mut Ui, speed: f32, value: &mut Vec3) -> InspectorResponse {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..3])
    }

    pub fn drag_float2(ui: &mut Ui, speed: f32, value: &mut Vec2) -> InspectorResponse {
        Self::drag_floatn(ui, speed, &mut value.data.as_mut_slice()[0..2])
    }

    pub fn drag_floatn(ui: &mut Ui, speed: f32, value: &mut [f32]) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        ui.horizontal_centered(|ui| {
            for value in value.iter_mut() {
                response |= ui.add(DragValue::new(value).speed(speed));
            }
        });
        response
    }

    pub fn drag_angle(ui: &mut Ui, value: &mut f32) -> InspectorResponse {
        let mut degrees = value.to_degrees();
        let res = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°"));
        if res.changed() {
            *value = degrees.to_radians();
        }
        InspectorResponse::from_response(&res)
    }

    pub fn drag_angle3(ui: &mut Ui, value: &mut Vec3) -> InspectorResponse {
        Self::drag_angle(ui, &mut value.x)
            | Self::drag_angle(ui, &mut value.y)
            | Self::drag_angle(ui, &mut value.z)
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

    fn run_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        mut draw: impl FnMut(&mut Ui) -> Response,
    ) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| rect = draw(ui).rect);
        });
        rect
    }

    fn click_events(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    #[test]
    fn asset_select_thumbnails_need_an_installed_handle_and_a_supported_type() {
        use calyx_engine::render::Shader;

        let ctx = egui::Context::default();
        let service = crate::widgets::ThumbnailService::default();
        let mut enabled = Vec::new();
        let mut check = |ctx: &egui::Context| {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    enabled.push((
                        Widgets::asset_select_thumbnails(ui, Some(Texture::type_uuid())).is_some(),
                        Widgets::asset_select_thumbnails(ui, None).is_some(),
                        Widgets::asset_select_thumbnails(ui, Some(Shader::type_uuid())).is_some(),
                    ));
                });
            });
        };

        check(&ctx);
        service.handle().install(&ctx);
        check(&ctx);

        assert_eq!(enabled, vec![(false, false, false), (true, true, false)]);
    }

    #[test]
    fn asset_select_requests_selected_and_listed_thumbnails() {
        use crate::widgets::{ThumbnailPriority, ThumbnailService, ThumbnailStatus};

        let root = std::env::temp_dir().join(format!("calyx-asset-select-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("failed to create asset root");
        let selected_path = root.join("selected.png");
        let listed_path = root.join("listed.png");
        std::fs::write(&selected_path, b"a").expect("failed to write texture source");
        std::fs::write(&listed_path, b"b").expect("failed to write texture source");

        let context =
            calyx_engine::test_support::test_asset_context_with_assets(vec![root.clone()]);
        let registry = context.registries.assets.read();
        let key = |path: &std::path::Path| {
            ThumbnailRequest::from_asset_path(&registry, path)
                .expect("texture should support thumbnails")
                .key()
        };
        let (selected_key, listed_key) = (key(&selected_path), key(&listed_path));
        let mut value = selected_key.asset_id;

        let service = ThumbnailService::default();
        let ctx = egui::Context::default();
        service.handle().install(&ctx);
        let mut draw = |ui: &mut Ui| {
            Widgets::asset_select(
                ui,
                &registry,
                "texture_select",
                Some(Texture::type_uuid()),
                &mut value,
            )
        };

        let combo = run_frame(&ctx, Vec::new(), &mut draw);
        assert_eq!(
            service.status(selected_key),
            ThumbnailStatus::Queued {
                priority: ThumbnailPriority::Normal
            }
        );
        assert_eq!(service.status(listed_key), ThumbnailStatus::Missing);

        run_frame(&ctx, click_events(combo.center()), &mut draw);
        run_frame(&ctx, Vec::new(), &mut draw);

        let high = ThumbnailStatus::Queued {
            priority: ThumbnailPriority::High,
        };
        assert_eq!(service.status(listed_key), high);
        assert_eq!(service.status(selected_key), high);

        drop(registry);
        std::fs::remove_dir_all(root).expect("failed to remove temp asset root");
    }

    #[test]
    fn search_select_focuses_search_field_when_opened() {
        let context = egui::Context::default();
        let mut state = SearchSelectState::default();
        state.open();
        let mut search_id = None;

        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let selector_id = ui.make_persistent_id("selector");
                search_id = Some(selector_id.with("search"));
                Widgets::search_select_contents(ui, "selector", &mut state, |_, _| {});
            });
        });

        assert!(context.memory(|memory| memory.has_focus(search_id.unwrap())));
        assert!(!state.should_request_focus);
    }

    #[test]
    fn texture_2d_filter_accepts_loaded_2d_texture() {
        let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
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
        let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
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

    #[test]
    fn texture_2d_filter_rejects_loaded_multisampled_texture() {
        let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
        let render_context = context.render_context.clone();
        let texture = context
            .registries
            .assets
            .read()
            .create(
                "test_texture_msaa".into(),
                Texture::new(
                    render_context,
                    &wgpu::TextureDescriptor {
                        label: Some("test_texture_msaa"),
                        size: wgpu::Extent3d {
                            width: 16,
                            height: 16,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 4,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    },
                    None,
                    Some(wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
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
