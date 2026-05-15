use super::geometry::{UiPoint, UiRect, UiSize};
use super::runtime::InteractionState;
use super::style::{
    AlignItems, JustifyContent, PointerEvents, ScreenClass, Style, StyleRegistry, Theme, UiLength,
};
use super::widgets::{ElementId, UiArena, UiLayoutKind, UiNodeHandle, WidgetHandle};

/// Final layout node used for hit testing and paint generation.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub id: Option<ElementId>,
    pub widget: WidgetHandle,
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
    arena: &UiArena,
    root: UiNodeHandle,
    viewport: UiRect,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
) -> LayoutNode {
    let screen_class = ScreenClass::from_width(viewport.width());
    layout_node(
        arena,
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
    arena: &UiArena,
    node: UiNodeHandle,
    constraints: Constraints,
    origin: UiPoint,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> LayoutNode {
    let node_data = arena.node(node);
    let widget = arena.widget(node_data.widget);
    let style = resolve_style(arena, node, theme, styles, state, screen_class);
    let margin_origin = UiPoint::new(origin.x + style.margin.left, origin.y + style.margin.top);
    let margin_constraints = UiSize::new(
        (constraints.max.width - style.margin.horizontal()).max(0.0),
        (constraints.max.height - style.margin.vertical()).max(0.0),
    );
    let outer_size = resolve_size(arena, node_data.widget, &style, margin_constraints);
    let rect = UiRect::from_min_size(margin_origin, outer_size);
    let content_rect = rect.inset(style.padding);

    let children = match widget.layout_kind() {
        UiLayoutKind::Row => layout_flex(
            arena,
            node,
            content_rect,
            true,
            theme,
            styles,
            state,
            screen_class,
        ),
        UiLayoutKind::Column => layout_flex(
            arena,
            node,
            content_rect,
            false,
            theme,
            styles,
            state,
            screen_class,
        ),
        UiLayoutKind::Stack => node_data
            .children
            .iter()
            .copied()
            .map(|child| {
                layout_node(
                    arena,
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
        UiLayoutKind::Center => node_data
            .children
            .iter()
            .copied()
            .map(|child| {
                let child_style = resolve_style(arena, child, theme, styles, state, screen_class);
                let child_size = resolve_size(
                    arena,
                    arena.node(child).widget,
                    &child_style,
                    content_rect.size(),
                );
                let child_origin = UiPoint::new(
                    content_rect.min.x + (content_rect.width() - child_size.width).max(0.0) * 0.5,
                    content_rect.min.y + (content_rect.height() - child_size.height).max(0.0) * 0.5,
                );
                layout_node(
                    arena,
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
        _ => node_data
            .children
            .iter()
            .copied()
            .map(|child| {
                layout_node(
                    arena,
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
        id: node_data.id.clone(),
        widget: node_data.widget,
        rect,
        content_rect,
        style,
        stop_propagation: node_data.stop_propagation.clone(),
        children,
    }
}

pub fn resolve_style(
    arena: &UiArena,
    node: UiNodeHandle,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> Style {
    let node = arena.node(node);
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
    if let Some(id) = node.id.as_ref() {
        let hover_amount = if style.transition_duration <= f32::EPSILON {
            if state.hovered.contains(id) {
                1.0
            } else {
                0.0
            }
        } else {
            state.hover_transition(id)
        };
        if hover_amount > 0.0 {
            let base = style.clone();
            let mut target = base.clone();
            node.hover_style.apply_to(&mut target);
            style = Style::lerp(&base, &target, ease_transition(hover_amount));
        }

        let pressed_amount = if style.transition_duration <= f32::EPSILON {
            if state.pressed.as_ref() == Some(id) {
                1.0
            } else {
                0.0
            }
        } else {
            state.pressed_transition(id)
        };
        if pressed_amount > 0.0 {
            let base = style.clone();
            let mut target = base.clone();
            node.pressed_style.apply_to(&mut target);
            style = Style::lerp(&base, &target, ease_transition(pressed_amount));
        }
    }
    style.opacity = style.opacity.clamp(0.0, 1.0);
    style
}

fn ease_transition(amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0);
    amount * amount * (3.0 - 2.0 * amount)
}

fn layout_flex(
    arena: &UiArena,
    node: UiNodeHandle,
    content_rect: UiRect,
    horizontal: bool,
    theme: &Theme,
    styles: &StyleRegistry,
    state: &InteractionState,
    screen_class: ScreenClass,
) -> Vec<LayoutNode> {
    let node_data = arena.node(node);
    let parent_style = resolve_style(arena, node, theme, styles, state, screen_class);
    let count = node_data.children.len();
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
    let child_styles: Vec<_> = node_data
        .children
        .iter()
        .copied()
        .map(|child| resolve_style(arena, child, theme, styles, state, screen_class))
        .collect();
    for (child, style) in node_data.children.iter().copied().zip(child_styles.iter()) {
        let child_size = default_size(
            arena,
            arena.node(child).widget,
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
    let space_between = if parent_style.justify_content == JustifyContent::SpaceBetween
        && count > 1
        && flex_total <= 0.0
    {
        (main_available - used_main).max(0.0) / count.saturating_sub(1) as f32
    } else {
        0.0
    };
    let mut cursor = match parent_style.justify_content {
        JustifyContent::Center => (main_available - used_main).max(0.0) * 0.5,
        JustifyContent::End => (main_available - used_main).max(0.0),
        _ => 0.0,
    };

    node_data
        .children
        .iter()
        .copied()
        .zip(child_styles.iter())
        .map(|(child, style)| {
            let default = default_size(
                arena,
                arena.node(child).widget,
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
            cursor += main + parent_style.gap + space_between;
            layout_node(
                arena,
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

fn resolve_size(arena: &UiArena, widget: WidgetHandle, style: &Style, max: UiSize) -> UiSize {
    let default = default_size(arena, widget, style, max);
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

fn default_size(arena: &UiArena, widget: WidgetHandle, style: &Style, max: UiSize) -> UiSize {
    arena.widget(widget).default_size(style, max)
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

/// Returns every element under `point` for hover styling and hover response state.
///
/// Hover intentionally ignores pointer event blocking so overlays can react while also
/// allowing background elements beneath them to react to hover.
pub fn hover_hit_ids(root: &LayoutNode, point: super::geometry::UiPoint) -> Vec<ElementId> {
    let mut ids = Vec::new();
    collect_hover_hits(root, point, &mut ids);
    ids
}

fn collect_hover_hits(
    node: &LayoutNode,
    point: super::geometry::UiPoint,
    ids: &mut Vec<ElementId>,
) {
    if !node.rect.contains(point) {
        return;
    }
    if let Some(id) = &node.id {
        ids.push(id.clone());
    }
    for child in &node.children {
        collect_hover_hits(child, point, ids);
    }
}
