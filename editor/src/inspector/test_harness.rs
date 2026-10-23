//! Drives inspectors through a headless egui context with simulated pointer
//! and keyboard input, and finds their widgets through the AccessKit tree.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, Vec2};
use engine::component::ComponentTransform;
use engine::context::{AssetContext, ReadOnlyRegistryContext};
use engine::reflect::TypeInfo;
use engine::scene::{GameObject, Scene};
use engine::utils::TypeUuid;

use super::inspector_registry::InspectorRegistry;
use super::type_inspector::{InspectorContext, InspectorResponse};

const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

/// One widget in the last frame's AccessKit tree.
#[derive(Clone, Debug)]
pub(crate) struct WidgetNode {
    pub role: Role,
    pub label: Option<String>,
    pub rect: Rect,
}

pub(crate) struct InspectorHarness {
    pub ctx: egui::Context,
    pub asset_context: AssetContext,
    pub registries: ReadOnlyRegistryContext,
    pub scene: Scene,
    pub game_object: GameObject,
    /// The reflected struct the drawn value's attributes come from.
    pub type_info: uuid::Uuid,
    time: f64,
    events: Vec<Event>,
    nodes: Vec<WidgetNode>,
    pointer: Pos2,
}

/// Draws one frame of an inspector and returns what it reported.
pub(crate) type Draw<'a> = dyn FnMut(&mut Ui, &InspectorContext) -> InspectorResponse + 'a;

impl InspectorHarness {
    pub fn new() -> Self {
        let asset_context = engine::test_support::test_asset_context_with_assets(Vec::new());
        let registries = asset_context.registries.lock_read();
        let mut scene = registries.scene();
        let game_object = scene.create(None, None);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            asset_context,
            registries,
            scene,
            game_object,
            type_info: ComponentTransform::type_uuid(),
            time: 0.0,
            events: Vec::new(),
            nodes: Vec::new(),
            pointer: Pos2::ZERO,
        }
    }

    /// The inspectors registered for the harness's types.
    pub fn inspector_registry(&self) -> InspectorRegistry {
        InspectorRegistry::new(&self.registries.types.read())
    }

    /// Runs one frame with the queued input and returns what `draw`
    /// reported.
    pub fn frame(&mut self, draw: &mut Draw) -> InspectorResponse {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let types = self.registries.types.read();
        let Some(TypeInfo::Struct(type_info)) = types.type_info_by_id(self.type_info) else {
            panic!("the harness type info should be a reflected struct");
        };
        let scene = self.scene.view();
        let ctx = InspectorContext {
            assets: &self.registries,
            scene: &scene,
            game_object: self.game_object,
            parent: None,
            type_info,
            field_name: None,
        };
        let mut response = InspectorResponse::NONE;
        let output = self.ctx.run(input, |egui_ctx| {
            egui::CentralPanel::default().show(egui_ctx, |ui| {
                re_ui::list_item::list_item_scope(ui, "inspector_test", |ui| {
                    response = draw(ui, &ctx);
                });
            });
        });
        drop(types);
        if let Some(update) = output.platform_output.accesskit_update {
            let mut nodes = update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    Some(WidgetNode {
                        role: node.role(),
                        label: node.label().map(str::to_owned),
                        rect: Rect::from_min_max(
                            Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                            Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                        ),
                    })
                })
                .collect::<Vec<_>>();
            nodes.sort_by(|a, b| {
                (a.rect.min.y, a.rect.min.x)
                    .partial_cmp(&(b.rect.min.y, b.rect.min.x))
                    .unwrap()
            });
            self.nodes = nodes;
        }
        response
    }

    /// Runs `count` frames without input and returns what each reported.
    pub fn idle(&mut self, count: usize, draw: &mut Draw) -> Vec<InspectorResponse> {
        (0..count).map(|_| self.frame(draw)).collect()
    }

    /// The widgets with `role` in the last frame, top to bottom, left to
    /// right.
    pub fn widgets(&self, role: Role) -> Vec<WidgetNode> {
        self.nodes
            .iter()
            .filter(|node| node.role == role)
            .cloned()
            .collect()
    }

    /// The `index`th widget with `role` in the last frame.
    pub fn widget(&self, role: Role, index: usize) -> Pos2 {
        let widgets = self.widgets(role);
        widgets
            .get(index)
            .unwrap_or_else(|| panic!("no {role:?} widget {index} in {:#?}", self.nodes))
            .rect
            .center()
    }

    /// The widget with `role` whose label is `label` in the last frame.
    pub fn labelled(&self, role: Role, label: &str) -> Pos2 {
        self.nodes
            .iter()
            .find(|node| node.role == role && node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no {role:?} widget labelled {label} in {:#?}", self.nodes))
            .rect
            .center()
    }

    fn move_to(&mut self, pos: Pos2) {
        self.pointer = pos;
        self.events.push(Event::PointerMoved(pos));
    }

    fn button(&mut self, pressed: bool) {
        self.events.push(Event::PointerButton {
            pos: self.pointer,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }

    /// Clicks at `pos` and returns what each frame of the click reported.
    pub fn click(&mut self, pos: Pos2, draw: &mut Draw) -> Vec<InspectorResponse> {
        let mut responses = Vec::new();
        self.move_to(pos);
        responses.push(self.frame(draw));
        self.button(true);
        responses.push(self.frame(draw));
        self.button(false);
        responses.push(self.frame(draw));
        responses.extend(self.idle(2, draw));
        responses
    }

    /// Drags from `pos` by `delta` over several frames and returns what each
    /// frame of the drag reported.
    pub fn drag(&mut self, pos: Pos2, delta: Vec2, draw: &mut Draw) -> Vec<InspectorResponse> {
        let mut responses = Vec::new();
        self.move_to(pos);
        responses.push(self.frame(draw));
        self.button(true);
        responses.push(self.frame(draw));
        for step in 1..=4 {
            self.move_to(pos + delta * (step as f32 / 4.0));
            responses.push(self.frame(draw));
        }
        self.button(false);
        responses.push(self.frame(draw));
        responses.extend(self.idle(2, draw));
        responses
    }

    /// Drags from `pos` through each offset in `offsets`, one frame each,
    /// then releases, and returns what each frame reported.
    pub fn drag_through(
        &mut self,
        pos: Pos2,
        offsets: &[Vec2],
        draw: &mut Draw,
    ) -> Vec<InspectorResponse> {
        let mut responses = Vec::new();
        self.move_to(pos);
        responses.push(self.frame(draw));
        self.button(true);
        responses.push(self.frame(draw));
        for offset in offsets {
            self.move_to(pos + *offset);
            responses.push(self.frame(draw));
        }
        self.button(false);
        responses.push(self.frame(draw));
        responses.extend(self.idle(2, draw));
        responses
    }

    /// Types `text` into the focused widget and returns what each frame
    /// reported.
    pub fn type_text(&mut self, text: &str, draw: &mut Draw) -> Vec<InspectorResponse> {
        self.events.push(Event::Text(text.to_owned()));
        let mut responses = vec![self.frame(draw)];
        responses.extend(self.idle(1, draw));
        responses
    }
}

/// Every response of a sequence combined.
pub(crate) fn combined(responses: &[InspectorResponse]) -> InspectorResponse {
    responses
        .iter()
        .fold(InspectorResponse::NONE, |all, response| all | *response)
}

/// Whether any frame of a sequence reported a change.
pub(crate) fn any_changed(responses: &[InspectorResponse]) -> bool {
    responses.iter().any(|response| response.changed)
}
