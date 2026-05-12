use super::geometry::{UiPoint, UiRect, UiSize};
use super::runtime::InteractionState;
use super::style::{
    AlignItems, JustifyContent, PointerEvents, ScreenClass, Style, StyleRegistry, Theme, UiLength,
};
use super::widgets::{ElementId, UiNode, WidgetKind};

/// Final layout node used for hit testing and paint generation.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub id: Option<ElementId>,
    pub kind: WidgetKind,
    pub rect: UiRect,
    pub content_rect: UiRect,
    pub style: Style,
    pub stop_propagation: Vec<super::events::PointerEventKind>,
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    pub fn find(&self, id: &ElementId) -> Option<&LayoutNode> {
        if self.id.as_ref() == Some(id) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(id))
    }
}

#[derive(Clone, Copy)]
struct Constraints {
    max: UiSize,
}

pub fn layout_tree(
    root: &UiNode,
    viewport: UiRect,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
) -> LayoutNode {
    let screen_class = ScreenClass::from_width(viewport.width());
    layout_node(
        root,
        Constraints {
            max: viewport.size(),
        },
        viewport.min,
        theme,
        styles,
        state,
        screen_class,
    )
}

fn layout_node(
    node: &UiNode,
    constraints: Constraints,
    origin: UiPoint,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> LayoutNode {
    let style = resolve_style(node, theme, styles, state, screen_class);
    let outer_size = resolve_size(&node.kind, &style, constraints.max);
    let rect = UiRect::from_min_size(origin, outer_size);
    let content_rect = rect.inset(style.padding);

    let children = match node.kind {
        WidgetKind::Row => {
            layout_flex(node, content_rect, true, theme, styles, state, screen_class)
        }
        WidgetKind::Column => layout_flex(
            node,
            content_rect,
            false,
            theme,
            styles,
            state,
            screen_class,
        ),
        WidgetKind::Stack => node
            .children
            .iter()
            .map(|child| {
                layout_node(
                    child,
                    Constraints {
                        max: content_rect.size(),
                    },
                    content_rect.min,
                    theme,
                    styles,
                    state,
                    screen_class,
                )
            })
            .collect(),
        WidgetKind::Center => node
            .children
            .iter()
            .map(|child| {
                let child_style = resolve_style(child, theme, styles, state, screen_class);
                let child_size = resolve_size(&child.kind, &child_style, content_rect.size());
                let child_origin = UiPoint::new(
                    content_rect.min.x + (content_rect.width() - child_size.width).max(0.0) * 0.5,
                    content_rect.min.y + (content_rect.height() - child_size.height).max(0.0) * 0.5,
                );
                layout_node(
                    child,
                    Constraints { max: child_size },
                    child_origin,
                    theme,
                    styles,
                    state,
                    screen_class,
                )
            })
            .collect(),
        _ => node
            .children
            .iter()
            .map(|child| {
                layout_node(
                    child,
                    Constraints {
                        max: content_rect.size(),
                    },
                    content_rect.min,
                    theme,
                    styles,
                    state,
                    screen_class,
                )
            })
            .collect(),
    };

    LayoutNode {
        id: node.id.clone(),
        kind: node.kind.clone(),
        rect,
        content_rect,
        style,
        stop_propagation: node.stop_propagation.clone(),
        children,
    }
}

pub fn resolve_style(
    node: &UiNode,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> Style {
    let mut style = theme.default_style.clone();
    if let Some(class) = &node.class {
        if let Some(preset) = styles.get(class) {
            preset.apply_to(&mut style);
        }
    }
    node.style.apply_to(&mut style);
    for (target, patch) in &node.responsive_styles {
        if *target == screen_class {
            patch.apply_to(&mut style);
        }
    }
    if node
        .id
        .as_ref()
        .is_some_and(|id| state.hovered.as_ref() == Some(id))
    {
        node.hover_style.apply_to(&mut style);
    }
    if node
        .id
        .as_ref()
        .is_some_and(|id| state.pressed.as_ref() == Some(id))
    {
        node.pressed_style.apply_to(&mut style);
    }
    style.opacity = style.opacity.clamp(0.0, 1.0);
    style
}

fn layout_flex(
    node: &UiNode,
    content_rect: UiRect,
    horizontal: bool,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> Vec<LayoutNode> {
    let parent_style = resolve_style(node, theme, styles, state, screen_class);
    let count = node.children.len();
    if count == 0 {
        return Vec::new();
    }
    let gap_total = parent_style.gap * count.saturating_sub(1) as f32;
    let main_available = if horizontal {
        content_rect.width()
    } else {
        content_rect.height()
    };
    let cross_available = if horizontal {
        content_rect.height()
    } else {
        content_rect.width()
    };

    let mut fixed_total = 0.0;
    let mut flex_total = 0.0;
    let child_styles: Vec<_> = node
        .children
        .iter()
        .map(|child| resolve_style(child, theme, styles, state, screen_class))
        .collect();
    for (child, style) in node.children.iter().zip(child_styles.iter()) {
        let child_size = default_size(
            &child.kind,
            style,
            UiSize::new(main_available, cross_available),
        );
        let explicit_main = resolve_length(
            if horizontal {
                style.width
            } else {
                style.height
            },
            main_available,
        );
        let basis = resolve_length(style.flex_basis, main_available)
            .or(explicit_main)
            .unwrap_or(if horizontal {
                child_size.width
            } else {
                child_size.height
            });
        if matches!(
            if horizontal {
                style.width
            } else {
                style.height
            },
            UiLength::Fill
        ) || style.flex_grow > 0.0
        {
            flex_total += style.flex_grow.max(1.0);
        } else {
            fixed_total += basis;
        }
    }
    let remaining = (main_available - fixed_total - gap_total).max(0.0);
    let used_main = fixed_total + gap_total + if flex_total > 0.0 { remaining } else { 0.0 };
    let mut cursor = match parent_style.justify_content {
        JustifyContent::Center => (main_available - used_main).max(0.0) * 0.5,
        JustifyContent::End => (main_available - used_main).max(0.0),
        _ => 0.0,
    };

    node.children
        .iter()
        .zip(child_styles.iter())
        .map(|(child, style)| {
            let default = default_size(
                child.kind(),
                style,
                UiSize::new(main_available, cross_available),
            );
            let main = if matches!(
                if horizontal {
                    style.width
                } else {
                    style.height
                },
                UiLength::Fill
            ) || style.flex_grow > 0.0
            {
                remaining * style.flex_grow.max(1.0) / flex_total.max(1.0)
            } else {
                resolve_length(
                    if horizontal {
                        style.width
                    } else {
                        style.height
                    },
                    main_available,
                )
                .unwrap_or(if horizontal {
                    default.width
                } else {
                    default.height
                })
            };
            let cross = resolve_length(
                if horizontal {
                    style.height
                } else {
                    style.width
                },
                cross_available,
            )
            .unwrap_or(match parent_style.align_items {
                AlignItems::Stretch => cross_available,
                _ => {
                    if horizontal {
                        default.height
                    } else {
                        default.width
                    }
                }
            });
            let cross_offset = match parent_style.align_items {
                AlignItems::Center => (cross_available - cross).max(0.0) * 0.5,
                AlignItems::End => (cross_available - cross).max(0.0),
                _ => 0.0,
            };
            let origin = if horizontal {
                UiPoint::new(
                    content_rect.min.x + cursor,
                    content_rect.min.y + cross_offset,
                )
            } else {
                UiPoint::new(
                    content_rect.min.x + cross_offset,
                    content_rect.min.y + cursor,
                )
            };
            let size = if horizontal {
                UiSize::new(main, cross)
            } else {
                UiSize::new(cross, main)
            };
            cursor += main + parent_style.gap;
            layout_node(
                child,
                Constraints { max: size },
                origin,
                theme,
                styles,
                state,
                screen_class,
            )
        })
        .collect()
}

trait WidgetKindRef {
    fn kind(&self) -> &WidgetKind;
}

impl WidgetKindRef for UiNode {
    fn kind(&self) -> &WidgetKind {
        &self.kind
    }
}

fn resolve_size(kind: &WidgetKind, style: &Style, max: UiSize) -> UiSize {
    let default = default_size(kind, style, max);
    let width = resolve_length(style.width, max.width).unwrap_or(default.width);
    let height = resolve_length(style.height, max.height).unwrap_or(default.height);
    apply_constraints(UiSize::new(width, height), style, max)
}

fn apply_constraints(mut size: UiSize, style: &Style, max: UiSize) -> UiSize {
    if let Some(value) = resolve_length(style.min_width, max.width) {
        size.width = size.width.max(value);
    }
    if let Some(value) = resolve_length(style.min_height, max.height) {
        size.height = size.height.max(value);
    }
    if let Some(value) = resolve_length(style.max_width, max.width) {
        size.width = size.width.min(value);
    }
    if let Some(value) = resolve_length(style.max_height, max.height) {
        size.height = size.height.min(value);
    }
    size.width = size.width.min(max.width).max(0.0);
    size.height = size.height.min(max.height).max(0.0);
    size
}

fn resolve_length(length: UiLength, parent: f32) -> Option<f32> {
    match length {
        UiLength::Auto => None,
        UiLength::Px(value) => Some(value.max(0.0)),
        UiLength::Percent(value) => Some(parent * value.clamp(0.0, 1.0)),
        UiLength::Fill => Some(parent.max(0.0)),
    }
}

fn default_size(kind: &WidgetKind, style: &Style, max: UiSize) -> UiSize {
    match kind {
        WidgetKind::Text { text } => UiSize::new(
            text.len() as f32 * style.font_size * 0.5,
            style.font_size * 1.25,
        ),
        WidgetKind::Button { label } => UiSize::new(
            label.len() as f32 * style.font_size * 0.55 + style.padding.horizontal(),
            style.font_size * 1.4 + style.padding.vertical(),
        ),
        WidgetKind::ProgressBar { .. } => {
            UiSize::new(160.0_f32.min(max.width), 16.0_f32.min(max.height))
        }
        WidgetKind::Crosshair { size, .. } => UiSize::new(*size, *size),
        WidgetKind::Spacer => UiSize::ZERO,
        WidgetKind::Row
        | WidgetKind::Column
        | WidgetKind::Stack
        | WidgetKind::Container
        | WidgetKind::Center => max,
        WidgetKind::SizedBox | WidgetKind::Image { .. } | WidgetKind::CustomPaint { .. } => {
            UiSize::new(0.0, 0.0)
        }
    }
}

/// Returns the deepest interactive path under `point`, front-most children first.
pub fn hit_test_path(root: &LayoutNode, point: super::geometry::UiPoint) -> Option<Vec<ElementId>> {
    if !root.rect.contains(point) {
        return None;
    }
    for child in root.children.iter().rev() {
        if let Some(mut path) = hit_test_path(child, point) {
            if let Some(id) = &root.id {
                path.insert(0, id.clone());
            }
            return Some(path);
        }
    }
    if root.style.pointer_events == PointerEvents::Auto {
        root.id.as_ref().map(|id| vec![id.clone()])
    } else {
        None
    }
}
