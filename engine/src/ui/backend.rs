use super::geometry::{UiPoint, UiRect};
use super::paint::PaintCommand;
use super::style::{Border, CornerRadius, UiColor, UiTransform};

/// Rendering backend boundary for runtime UI paint commands.
pub trait UiBackend {
    type TextureId: Clone;

    fn begin_frame(&mut self, viewport: UiRect, scale_factor: f32);
    fn push_clip(&mut self, rect: UiRect);
    fn pop_clip(&mut self);
    fn push_transform(&mut self, rect: UiRect, transform: UiTransform);
    fn pop_transform(&mut self);
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
    PushTransform {
        rect: UiRect,
        transform: UiTransform,
    },
    PopTransform,
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

    fn push_transform(&mut self, rect: UiRect, transform: UiTransform) {
        self.ops.push(BackendOp::PushTransform { rect, transform });
    }

    fn pop_transform(&mut self) {
        self.ops.push(BackendOp::PopTransform);
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
            PaintCommand::PushTransform { rect, transform } => {
                backend.push_transform(*rect, *transform)
            }
            PaintCommand::PopTransform => backend.pop_transform(),
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
    transform_stack: Vec<(UiRect, UiTransform)>,
}

impl<'a> EguiUiBackend<'a> {
    pub fn new(painter: &'a egui::Painter) -> Self {
        Self {
            painter,
            clip_stack: Vec::new(),
            transform_stack: Vec::new(),
        }
    }
}

impl UiBackend for EguiUiBackend<'_> {
    type TextureId = egui::TextureId;

    fn begin_frame(&mut self, _viewport: UiRect, _scale_factor: f32) {}

    fn push_clip(&mut self, rect: UiRect) {
        let clip = to_egui_rect(rect);
        let clip = self
            .clip_stack
            .last()
            .map(|parent| intersect_clip_rect(*parent, clip))
            .unwrap_or(clip);
        self.clip_stack.push(clip);
    }

    fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    fn push_transform(&mut self, rect: UiRect, transform: UiTransform) {
        self.transform_stack.push((rect, transform));
    }

    fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }

    fn fill_rect(&mut self, rect: UiRect, color: UiColor, radius: CornerRadius) {
        if self.has_transform() {
            self.draw_transformed_polygon(rect, to_egui_color(color), None);
            return;
        }
        self.active_painter().rect_filled(
            to_egui_rect(rect),
            to_egui_radius(radius),
            to_egui_color(color),
        );
    }

    fn stroke_rect(&mut self, rect: UiRect, border: Border, radius: CornerRadius) {
        if self.has_transform() {
            self.stroke_transformed_polygon(rect, border);
            return;
        }
        self.active_painter().rect_stroke(
            to_egui_rect(rect),
            to_egui_radius(radius),
            egui::Stroke::new(border.width, to_egui_color(border.color)),
            egui::StrokeKind::Middle,
        );
    }

    fn draw_text(&mut self, rect: UiRect, text: &str, color: UiColor, font_size: f32) {
        let pos = self.transform_point(rect.min);
        self.active_painter().text(
            egui::pos2(pos.x, pos.y),
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
        if self.has_transform() {
            self.draw_transformed_image(rect, texture, tint);
            return;
        }
        self.active_painter().image(
            texture,
            to_egui_rect(rect),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            to_egui_color(tint),
        );
    }

    fn draw_line(&mut self, start: UiPoint, end: UiPoint, color: UiColor, width: f32) {
        let start = self.transform_point(start);
        let end = self.transform_point(end);
        self.active_painter().line_segment(
            [egui::pos2(start.x, start.y), egui::pos2(end.x, end.y)],
            egui::Stroke::new(width, to_egui_color(color)),
        );
    }

    fn end_frame(&mut self) {
        self.clip_stack.clear();
        self.transform_stack.clear();
    }
}

impl EguiUiBackend<'_> {
    fn active_painter(&self) -> egui::Painter {
        self.clip_stack
            .last()
            .map(|clip| self.painter.with_clip_rect(*clip))
            .unwrap_or_else(|| self.painter.clone())
    }

    fn has_transform(&self) -> bool {
        !self.transform_stack.is_empty()
    }

    fn transform_point(&self, point: UiPoint) -> UiPoint {
        self.transform_stack
            .iter()
            .fold(point, |point, (rect, transform)| {
                project_fake_3d(point, *rect, *transform)
            })
    }

    fn transformed_corners(&self, rect: UiRect) -> [egui::Pos2; 4] {
        [
            self.transform_point(rect.min),
            self.transform_point(UiPoint::new(rect.max.x, rect.min.y)),
            self.transform_point(rect.max),
            self.transform_point(UiPoint::new(rect.min.x, rect.max.y)),
        ]
        .map(|point| egui::pos2(point.x, point.y))
    }

    fn draw_transformed_polygon(
        &self,
        rect: UiRect,
        fill: egui::Color32,
        stroke: Option<egui::Stroke>,
    ) {
        let points = self.transformed_corners(rect).to_vec();
        self.active_painter().add(egui::Shape::convex_polygon(
            points,
            fill,
            stroke.unwrap_or_default(),
        ));
    }

    fn stroke_transformed_polygon(&self, rect: UiRect, border: Border) {
        let points = self.transformed_corners(rect);
        let stroke = egui::Stroke::new(border.width, to_egui_color(border.color));
        let painter = self.active_painter();
        for (start, end) in points
            .iter()
            .copied()
            .zip(points.iter().copied().cycle().skip(1))
            .take(points.len())
        {
            painter.line_segment([start, end], stroke);
        }
    }

    fn draw_transformed_image(&self, rect: UiRect, texture: egui::TextureId, tint: UiColor) {
        let points = self.transformed_corners(rect);
        let uv = [
            egui::pos2(0.0, 0.0),
            egui::pos2(1.0, 0.0),
            egui::pos2(1.0, 1.0),
            egui::pos2(0.0, 1.0),
        ];
        let mut mesh = egui::epaint::Mesh::with_texture(texture);
        let color = to_egui_color(tint);
        let base = mesh.vertices.len() as u32;
        for i in 0..4 {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: points[i],
                uv: uv[i],
                color,
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        self.active_painter().add(egui::Shape::mesh(mesh));
    }
}

fn project_fake_3d(point: UiPoint, rect: UiRect, transform: UiTransform) -> UiPoint {
    if transform.is_identity() {
        return point;
    }
    let center = UiPoint::new(
        rect.min.x + rect.width() * 0.5,
        rect.min.y + rect.height() * 0.5,
    );
    let mut x = (point.x - center.x) * transform.scale;
    let mut y = (point.y - center.y) * transform.scale;
    let (sin_y, cos_y) = transform.rotate_y.sin_cos();
    let (sin_x, cos_x) = transform.rotate_x.sin_cos();
    let (sin_z, cos_z) = transform.rotate_z.sin_cos();

    let z_y = x * sin_y;
    x *= cos_y;

    let y_rot = y * cos_x - z_y * sin_x;
    let z = y * sin_x + z_y * cos_x;
    y = y_rot;

    let x_rot = x * cos_z - y * sin_z;
    let y_rot = x * sin_z + y * cos_z;
    x = x_rot;
    y = y_rot;

    let perspective = transform.perspective.max(1.0);
    let scale = perspective / (perspective + z);
    UiPoint::new(center.x + x * scale, center.y + y * scale)
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

pub(crate) fn intersect_clip_rect(parent: egui::Rect, child: egui::Rect) -> egui::Rect {
    parent.intersect(child)
}
