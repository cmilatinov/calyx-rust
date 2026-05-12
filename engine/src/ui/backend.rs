use super::geometry::{UiPoint, UiRect};
use super::paint::PaintCommand;
use super::style::{Border, CornerRadius, UiColor};

/// Rendering backend boundary for runtime UI paint commands.
pub trait UiBackend {
    type TextureId: Clone;

    fn begin_frame(&mut self, viewport: UiRect, scale_factor: f32);
    fn push_clip(&mut self, rect: UiRect);
    fn pop_clip(&mut self);
    fn fill_rect(&mut self, rect: UiRect, color: UiColor, radius: CornerRadius);
    fn stroke_rect(&mut self, rect: UiRect, border: Border, radius: CornerRadius);
    fn draw_text(&mut self, rect: UiRect, text: &str, color: UiColor, font_size: f32);
    fn draw_image(
        &mut self,
        rect: UiRect,
        texture: Self::TextureId,
        tint: UiColor,
        radius: CornerRadius,
    );
    fn draw_line(&mut self, start: UiPoint, end: UiPoint, color: UiColor, width: f32);
    fn end_frame(&mut self);
}

#[derive(Clone, Debug, PartialEq)]
pub enum BackendOp<T> {
    BeginFrame {
        viewport: UiRect,
        scale_factor: f32,
    },
    PushClip(UiRect),
    PopClip,
    FillRect {
        rect: UiRect,
        color: UiColor,
        radius: CornerRadius,
    },
    StrokeRect {
        rect: UiRect,
        border: Border,
        radius: CornerRadius,
    },
    Text {
        rect: UiRect,
        text: String,
        color: UiColor,
        font_size: f32,
    },
    Image {
        rect: UiRect,
        texture: T,
        tint: UiColor,
        radius: CornerRadius,
    },
    Line {
        start: UiPoint,
        end: UiPoint,
        color: UiColor,
        width: f32,
    },
    EndFrame,
}

/// Test backend that records translated operations.
#[derive(Clone, Debug, Default)]
pub struct RecordingBackend<T: Clone> {
    pub ops: Vec<BackendOp<T>>,
}

impl<T: Clone> UiBackend for RecordingBackend<T> {
    type TextureId = T;

    fn begin_frame(&mut self, viewport: UiRect, scale_factor: f32) {
        self.ops.push(BackendOp::BeginFrame {
            viewport,
            scale_factor,
        });
    }

    fn push_clip(&mut self, rect: UiRect) {
        self.ops.push(BackendOp::PushClip(rect));
    }

    fn pop_clip(&mut self) {
        self.ops.push(BackendOp::PopClip);
    }

    fn fill_rect(&mut self, rect: UiRect, color: UiColor, radius: CornerRadius) {
        self.ops.push(BackendOp::FillRect {
            rect,
            color,
            radius,
        });
    }

    fn stroke_rect(&mut self, rect: UiRect, border: Border, radius: CornerRadius) {
        self.ops.push(BackendOp::StrokeRect {
            rect,
            border,
            radius,
        });
    }

    fn draw_text(&mut self, rect: UiRect, text: &str, color: UiColor, font_size: f32) {
        self.ops.push(BackendOp::Text {
            rect,
            text: text.to_owned(),
            color,
            font_size,
        });
    }

    fn draw_image(
        &mut self,
        rect: UiRect,
        texture: Self::TextureId,
        tint: UiColor,
        radius: CornerRadius,
    ) {
        self.ops.push(BackendOp::Image {
            rect,
            texture,
            tint,
            radius,
        });
    }

    fn draw_line(&mut self, start: UiPoint, end: UiPoint, color: UiColor, width: f32) {
        self.ops.push(BackendOp::Line {
            start,
            end,
            color,
            width,
        });
    }

    fn end_frame(&mut self) {
        self.ops.push(BackendOp::EndFrame);
    }
}

pub fn render_commands<B, F>(
    backend: &mut B,
    viewport: UiRect,
    scale_factor: f32,
    commands: &[PaintCommand],
    mut texture_lookup: F,
) where
    B: UiBackend,
    F: FnMut(&str) -> B::TextureId,
{
    backend.begin_frame(viewport, scale_factor);
    for command in commands {
        match command {
            PaintCommand::PushClip(rect) => backend.push_clip(*rect),
            PaintCommand::PopClip => backend.pop_clip(),
            PaintCommand::FillRect {
                rect,
                color,
                radius,
            } => backend.fill_rect(*rect, *color, *radius),
            PaintCommand::StrokeRect {
                rect,
                border,
                radius,
            } => backend.stroke_rect(*rect, *border, *radius),
            PaintCommand::Text {
                rect,
                text,
                color,
                font_size,
            } => backend.draw_text(*rect, text, *color, *font_size),
            PaintCommand::Image {
                rect,
                texture,
                tint,
                radius,
            } => backend.draw_image(*rect, texture_lookup(texture), *tint, *radius),
            PaintCommand::Line {
                start,
                end,
                color,
                width,
            } => backend.draw_line(*start, *end, *color, *width),
            PaintCommand::Custom(_) => {}
        }
    }
    backend.end_frame();
}

/// egui-backed adapter for the initial runtime UI backend.
pub struct EguiUiBackend<'a> {
    painter: &'a egui::Painter,
    clip_stack: Vec<egui::Rect>,
}

impl<'a> EguiUiBackend<'a> {
    pub fn new(painter: &'a egui::Painter) -> Self {
        Self {
            painter,
            clip_stack: Vec::new(),
        }
    }
}

impl UiBackend for EguiUiBackend<'_> {
    type TextureId = egui::TextureId;

    fn begin_frame(&mut self, _viewport: UiRect, _scale_factor: f32) {}

    fn push_clip(&mut self, rect: UiRect) {
        self.clip_stack.push(to_egui_rect(rect));
    }

    fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    fn fill_rect(&mut self, rect: UiRect, color: UiColor, radius: CornerRadius) {
        self.active_painter().rect_filled(
            to_egui_rect(rect),
            to_egui_radius(radius),
            to_egui_color(color),
        );
    }

    fn stroke_rect(&mut self, rect: UiRect, border: Border, radius: CornerRadius) {
        self.active_painter().rect_stroke(
            to_egui_rect(rect),
            to_egui_radius(radius),
            egui::Stroke::new(border.width, to_egui_color(border.color)),
            egui::StrokeKind::Middle,
        );
    }

    fn draw_text(&mut self, rect: UiRect, text: &str, color: UiColor, font_size: f32) {
        self.active_painter().text(
            to_egui_rect(rect).left_top(),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(font_size),
            to_egui_color(color),
        );
    }

    fn draw_image(
        &mut self,
        rect: UiRect,
        texture: Self::TextureId,
        tint: UiColor,
        _radius: CornerRadius,
    ) {
        self.active_painter().image(
            texture,
            to_egui_rect(rect),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            to_egui_color(tint),
        );
    }

    fn draw_line(&mut self, start: UiPoint, end: UiPoint, color: UiColor, width: f32) {
        self.active_painter().line_segment(
            [egui::pos2(start.x, start.y), egui::pos2(end.x, end.y)],
            egui::Stroke::new(width, to_egui_color(color)),
        );
    }

    fn end_frame(&mut self) {
        self.clip_stack.clear();
    }
}

impl EguiUiBackend<'_> {
    fn active_painter(&self) -> egui::Painter {
        self.clip_stack
            .last()
            .map(|clip| self.painter.with_clip_rect(*clip))
            .unwrap_or_else(|| self.painter.clone())
    }
}

fn to_egui_rect(rect: UiRect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y),
        egui::pos2(rect.max.x, rect.max.y),
    )
}

fn to_egui_color(color: UiColor) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

fn to_egui_radius(radius: CornerRadius) -> egui::CornerRadius {
    egui::CornerRadius {
        nw: radius.top_left.clamp(0.0, u8::MAX as f32) as u8,
        ne: radius.top_right.clamp(0.0, u8::MAX as f32) as u8,
        sw: radius.bottom_left.clamp(0.0, u8::MAX as f32) as u8,
        se: radius.bottom_right.clamp(0.0, u8::MAX as f32) as u8,
    }
}
