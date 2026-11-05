use super::geometry::{UiPoint, UiRect};
use super::layout::LayoutNode;
use super::style::{Border, CornerRadius, UiColor, UiTransform};
use super::widgets::UiArena;

/// Backend-neutral paint command.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintCommand {
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
    /// A convex polygon with perimeter-ordered vertices. Fewer than three are ignored.
    FillPolygon {
        points: Vec<UiPoint>,
        color: UiColor,
    },
    /// Joined triangles without antialiasing on internal edges. Invalid indices
    /// or an index count that is not a multiple of three discard the mesh.
    FillTriangleMesh {
        points: Vec<UiPoint>,
        indices: Vec<u32>,
        color: UiColor,
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

pub fn collect_paint_commands(arena: &UiArena, root: &LayoutNode) -> Vec<PaintCommand> {
    let mut commands = Vec::new();
    collect_node_paint(arena, root, &mut commands);
    commands
}

fn collect_node_paint(arena: &UiArena, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
    let transformed = !node.style.transform.is_identity();
    if transformed {
        commands.push(PaintCommand::PushTransform {
            rect: node.rect,
            transform: node.style.transform,
        });
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

    if node.style.clip {
        commands.push(PaintCommand::PushClip(node.content_rect));
    }

    arena.widget(node.widget).paint(node, commands);

    for child in &node.children {
        collect_node_paint(arena, child, commands);
    }

    if node.style.clip {
        commands.push(PaintCommand::PopClip);
    }
    if transformed {
        commands.push(PaintCommand::PopTransform);
    }
}
