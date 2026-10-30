//! Drives a widget through a headless egui context with simulated pointer
//! and keyboard input, and finds its parts through the AccessKit tree.
#![allow(dead_code)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Ui, Vec2};

const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

/// One labelled widget of the last frame.
#[derive(Clone, Debug)]
pub(crate) struct LabelledRect {
    pub label: String,
    pub rect: Rect,
}

/// Shows a widget in a bare `Ui`, frame by frame.
pub(crate) struct WidgetDriver {
    ctx: egui::Context,
    time: f64,
    events: Vec<Event>,
    nodes: Vec<LabelledRect>,
    pointer: Pos2,
}

impl WidgetDriver {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            time: 0.0,
            events: Vec::new(),
            nodes: Vec::new(),
            pointer: Pos2::ZERO,
        }
    }

    /// Runs one frame with the queued input and returns what `draw`
    /// returned.
    pub fn frame(&mut self, draw: &mut dyn FnMut(&mut Ui) -> Response) -> Response {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let mut response = None;
        let output = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| response = Some(draw(ui)));
        });
        if let Some(update) = output.platform_output.accesskit_update {
            self.nodes = update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    Some(LabelledRect {
                        label: node.label()?.trim().to_owned(),
                        rect: Rect::from_min_max(
                            Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                            Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
                        ),
                    })
                })
                .collect();
            self.nodes.sort_by(|a, b| {
                (a.rect.min.y, a.rect.min.x)
                    .partial_cmp(&(b.rect.min.y, b.rect.min.x))
                    .unwrap()
            });
        }
        response.expect("the widget was drawn")
    }

    /// Runs `count` frames without input.
    pub fn idle(
        &mut self,
        count: usize,
        draw: &mut dyn FnMut(&mut Ui) -> Response,
    ) -> Vec<Response> {
        (0..count).map(|_| self.frame(draw)).collect()
    }

    /// The labelled widgets of the last frame, top to bottom.
    pub fn labelled_widgets(&self) -> &[LabelledRect] {
        &self.nodes
    }

    /// The rectangle of the first widget labelled `label` in the last frame.
    pub fn labelled(&self, label: &str) -> Rect {
        self.nodes
            .iter()
            .find(|node| node.label == label)
            .unwrap_or_else(|| panic!("no widget labelled {label} in {:#?}", self.nodes))
            .rect
    }

    fn move_to(&mut self, pos: Pos2) {
        self.pointer = pos;
        self.events.push(Event::PointerMoved(pos));
    }

    fn press(&mut self, button: PointerButton, pressed: bool) {
        self.events.push(Event::PointerButton {
            pos: self.pointer,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }

    /// Clicks at `pos` with `button` and returns each frame's response.
    pub fn click_with(
        &mut self,
        button: PointerButton,
        pos: Pos2,
        draw: &mut dyn FnMut(&mut Ui) -> Response,
    ) -> Vec<Response> {
        let mut responses = Vec::new();
        self.move_to(pos);
        responses.push(self.frame(draw));
        self.press(button, true);
        responses.push(self.frame(draw));
        self.press(button, false);
        responses.push(self.frame(draw));
        responses.extend(self.idle(2, draw));
        responses
    }

    /// Clicks at `pos` and returns each frame's response.
    pub fn click(&mut self, pos: Pos2, draw: &mut dyn FnMut(&mut Ui) -> Response) -> Vec<Response> {
        self.click_with(PointerButton::Primary, pos, draw)
    }

    /// Clicks twice at `pos`, quickly enough to double-click.
    pub fn double_click(
        &mut self,
        pos: Pos2,
        draw: &mut dyn FnMut(&mut Ui) -> Response,
    ) -> Vec<Response> {
        let mut responses = self.click(pos, draw);
        responses.extend(self.click(pos, draw));
        responses
    }

    /// Presses and releases `key` and returns each frame's response.
    pub fn press_key(
        &mut self,
        key: Key,
        draw: &mut dyn FnMut(&mut Ui) -> Response,
    ) -> Vec<Response> {
        for pressed in [true, false] {
            self.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        let mut responses = vec![self.frame(draw)];
        responses.extend(self.idle(1, draw));
        responses
    }

    /// Drags from `pos` by `delta` over several frames and returns each
    /// frame's response.
    pub fn drag(
        &mut self,
        pos: Pos2,
        delta: Vec2,
        draw: &mut dyn FnMut(&mut Ui) -> Response,
    ) -> Vec<Response> {
        let mut responses = Vec::new();
        self.move_to(pos);
        responses.push(self.frame(draw));
        self.press(PointerButton::Primary, true);
        responses.push(self.frame(draw));
        for step in 1..=4 {
            self.move_to(pos + delta * (step as f32 / 4.0));
            responses.push(self.frame(draw));
        }
        self.press(PointerButton::Primary, false);
        responses.push(self.frame(draw));
        responses.extend(self.idle(2, draw));
        responses
    }
}
