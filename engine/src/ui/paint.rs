use super::geometry::{UiPoint, UiRect};
use super::layout::LayoutNode;
use super::style::{Border, CornerRadius, UiColor};
use super::widgets::WidgetKind;

/// Backend-neutral paint command.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintCommand {
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
        texture: String,
        tint: UiColor,
        radius: CornerRadius,
    },
    Line {
        start: UiPoint,
        end: UiPoint,
        color: UiColor,
        width: f32,
    },
    Custom(String),
}

pub fn collect_paint_commands(root: &LayoutNode) -> Vec<PaintCommand> {
    let mut commands = Vec::new();
    collect_node_paint(root, &mut commands);
    commands
}

fn collect_node_paint(node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
    if node.style.clip {
        commands.push(PaintCommand::PushClip(node.content_rect));
    }

    let background = node.style.background.with_alpha(node.style.opacity);
    if background.is_visible() {
        commands.push(PaintCommand::FillRect {
            rect: node.rect,
            color: background,
            radius: node.style.radius,
        });
    }
    if node.style.border.is_visible() {
        commands.push(PaintCommand::StrokeRect {
            rect: node.rect,
            border: node.style.border,
            radius: node.style.radius,
        });
    }

    match &node.kind {
        WidgetKind::Text { text } | WidgetKind::Button { label: text } => {
            commands.push(PaintCommand::Text {
                rect: node.content_rect,
                text: text.clone(),
                color: node.style.text_color.with_alpha(node.style.opacity),
                font_size: node.style.font_size,
            });
        }
        WidgetKind::Image { texture } => {
            commands.push(PaintCommand::Image {
                rect: node.content_rect,
                texture: texture.clone(),
                tint: node.style.text_color.with_alpha(node.style.opacity),
                radius: node.style.radius,
            });
        }
        WidgetKind::ProgressBar { value, fill } => {
            let fill_rect = UiRect {
                max: UiPoint::new(
                    node.content_rect.min.x + node.content_rect.width() * value.clamp(0.0, 1.0),
                    node.content_rect.max.y,
                ),
                ..node.content_rect
            };
            commands.push(PaintCommand::FillRect {
                rect: fill_rect,
                color: fill.with_alpha(node.style.opacity),
                radius: node.style.radius,
            });
        }
        WidgetKind::Crosshair { color, size } => {
            let center = UiPoint::new(
                node.rect.min.x + node.rect.width() * 0.5,
                node.rect.min.y + node.rect.height() * 0.5,
            );
            let half = *size * 0.5;
            commands.push(PaintCommand::Line {
                start: UiPoint::new(center.x - half, center.y),
                end: UiPoint::new(center.x + half, center.y),
                color: *color,
                width: 1.0,
            });
            commands.push(PaintCommand::Line {
                start: UiPoint::new(center.x, center.y - half),
                end: UiPoint::new(center.x, center.y + half),
                color: *color,
                width: 1.0,
            });
        }
        WidgetKind::CustomPaint { commands: custom } => commands.extend(custom.iter().cloned()),
        _ => {}
    }

    for child in &node.children {
        collect_node_paint(child, commands);
    }

    if node.style.clip {
        commands.push(PaintCommand::PopClip);
    }
}
