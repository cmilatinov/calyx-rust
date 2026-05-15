use std::collections::{HashMap, HashSet};

use super::events::{ElementResponse, EventPhase, PointerEventKind, UiEventRecord, UiInput};
use super::geometry::UiRect;
use super::layout::{hit_test_path, hover_hit_ids, layout_tree, resolve_style, LayoutNode};
use super::paint::{collect_paint_commands, PaintCommand};
use super::style::{ScreenClass, StyleRegistry, Theme};
use super::widgets::{ElementId, UiArena, UiNodeHandle};

const TRANSITION_EPSILON: f32 = 0.0001;

/// Cross-frame interaction state for a UI tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractionState {
    pub hovered: HashSet<ElementId>,
    pub pressed: Option<ElementId>,
    pub focused: Option<ElementId>,
    hover_transition: HashMap<ElementId, f32>,
    pressed_transition: HashMap<ElementId, f32>,
    captured: Option<ElementId>,
    previous_input: UiInput,
    drag_started: bool,
}

impl InteractionState {
    pub fn hover_transition(&self, id: &ElementId) -> f32 {
        self.hover_transition.get(id).copied().unwrap_or(0.0)
    }

    pub fn pressed_transition(&self, id: &ElementId) -> f32 {
        self.pressed_transition.get(id).copied().unwrap_or(0.0)
    }
}

/// Complete UI frame result.
#[derive(Clone, Debug)]
pub struct UiFrame {
    pub layout: LayoutNode,
    pub paint_commands: Vec<PaintCommand>,
    pub events: Vec<UiEventRecord>,
    pub responses: HashMap<ElementId, ElementResponse>,
    pub consumed_pointer: bool,
}

impl UiFrame {
    pub fn response(&self, id: impl Into<ElementId>) -> ElementResponse {
        self.responses.get(&id.into()).cloned().unwrap_or_default()
    }

    pub fn clicked(&self, id: impl Into<ElementId>) -> bool {
        self.response(id).clicked
    }

    pub fn hovered(&self, id: impl Into<ElementId>) -> bool {
        self.response(id).hovered
    }
}

/// Runtime that resolves one UI tree per frame.
#[derive(Clone, Debug, Default)]
pub struct UiRuntime {
    pub state: InteractionState,
    pub styles: StyleRegistry,
}

impl UiRuntime {
    pub fn with_styles(styles: StyleRegistry) -> Self {
        Self {
            styles,
            ..Default::default()
        }
    }

    pub fn frame(
        &mut self,
        arena: &UiArena,
        root: UiNodeHandle,
        viewport: UiRect,
        input: UiInput,
        theme: &Theme,
    ) -> UiFrame {
        let hit_layout = layout_tree(arena, root, viewport, theme, &self.styles, &self.state);
        let mut events = Vec::new();
        let mut responses: HashMap<ElementId, ElementResponse> = HashMap::new();
        let target_path = input
            .pointer_position
            .and_then(|point| hit_test_path(&hit_layout, point));
        let hover_ids = input
            .pointer_position
            .map(|point| hover_hit_ids(&hit_layout, point))
            .unwrap_or_default();
        let target = self
            .state
            .captured
            .clone()
            .or_else(|| target_path.as_ref().and_then(|path| path.last().cloned()));

        self.update_hover(&hover_ids, &mut events, &mut responses);

        if input.pointer_position != self.state.previous_input.pointer_position {
            if let Some(path) = target_path.as_ref() {
                dispatch_path(
                    &hit_layout,
                    path,
                    PointerEventKind::PointerMove,
                    &mut events,
                );
            }
        }

        if input.pointer_down && !self.state.previous_input.pointer_down {
            if let Some(path) = target_path.as_ref() {
                if let Some(id) = path.last() {
                    self.state.pressed = Some(id.clone());
                    self.state.captured = Some(id.clone());
                    self.state.focused = Some(id.clone());
                    responses.entry(id.clone()).or_default().pressed = true;
                }
                dispatch_path(
                    &hit_layout,
                    path,
                    PointerEventKind::PointerDown,
                    &mut events,
                );
            }
        }

        if input.pointer_down
            && self.state.previous_input.pointer_down
            && input.pointer_position != self.state.previous_input.pointer_position
        {
            if let Some(id) = target.as_ref() {
                let path = path_to_id(&hit_layout, id).unwrap_or_else(|| vec![id.clone()]);
                if !self.state.drag_started {
                    dispatch_path(&hit_layout, &path, PointerEventKind::DragStart, &mut events);
                    self.state.drag_started = true;
                }
                dispatch_path(&hit_layout, &path, PointerEventKind::Drag, &mut events);
                responses.entry(id.clone()).or_default().dragged = true;
            }
        }

        if !input.pointer_down && self.state.previous_input.pointer_down {
            if let Some(id) = self.state.pressed.clone() {
                let path = path_to_id(&hit_layout, &id).unwrap_or_else(|| vec![id.clone()]);
                dispatch_path(&hit_layout, &path, PointerEventKind::PointerUp, &mut events);
                if target_path
                    .as_ref()
                    .is_some_and(|path| path.last() == Some(&id))
                {
                    dispatch_path(&hit_layout, &path, PointerEventKind::Click, &mut events);
                    responses.entry(id.clone()).or_default().clicked = true;
                }
                if self.state.drag_started {
                    dispatch_path(&hit_layout, &path, PointerEventKind::DragEnd, &mut events);
                }
            }
            self.state.pressed = None;
            self.state.captured = None;
            self.state.drag_started = false;
        }

        for id in &self.state.hovered {
            responses.entry(id.clone()).or_default().hovered = true;
        }
        if let Some(id) = &self.state.pressed {
            responses.entry(id.clone()).or_default().pressed = true;
        }

        self.update_style_transitions(arena, root, viewport, theme, input.delta_time);
        self.state.previous_input = input;
        let layout = layout_tree(arena, root, viewport, theme, &self.styles, &self.state);
        let paint_commands = collect_paint_commands(arena, &layout);
        UiFrame {
            layout,
            paint_commands,
            events,
            responses,
            consumed_pointer: target_path.is_some(),
        }
    }

    fn update_hover(
        &mut self,
        next: &[ElementId],
        events: &mut Vec<UiEventRecord>,
        responses: &mut HashMap<ElementId, ElementResponse>,
    ) {
        let next: HashSet<_> = next.iter().cloned().collect();
        if self.state.hovered == next {
            return;
        }
        for id in self.state.hovered.difference(&next) {
            events.push(UiEventRecord {
                element_id: id.clone(),
                kind: PointerEventKind::PointerLeave,
                phase: EventPhase::Target,
            });
        }
        for id in next.difference(&self.state.hovered) {
            events.push(UiEventRecord {
                element_id: id.clone(),
                kind: PointerEventKind::PointerEnter,
                phase: EventPhase::Target,
            });
            responses.entry(id.clone()).or_default().hovered = true;
        }
        self.state.hovered = next;
    }

    fn update_style_transitions(
        &mut self,
        arena: &UiArena,
        root: UiNodeHandle,
        viewport: UiRect,
        theme: &Theme,
        delta_time: f32,
    ) {
        let screen_class = ScreenClass::from_width(viewport.width());
        let mut seen = HashSet::new();
        self.update_node_transitions(
            arena,
            root,
            theme,
            screen_class,
            delta_time.max(0.0),
            &mut seen,
        );
        self.state
            .hover_transition
            .retain(|id, amount| seen.contains(id) && *amount > 0.0);
        self.state
            .pressed_transition
            .retain(|id, amount| seen.contains(id) && *amount > 0.0);
    }

    fn update_node_transitions(
        &mut self,
        arena: &UiArena,
        node: UiNodeHandle,
        theme: &Theme,
        screen_class: ScreenClass,
        delta_time: f32,
        seen: &mut HashSet<ElementId>,
    ) {
        let node_data = arena.node(node);
        let id = node_data.id.clone();
        let children = node_data.children.clone();

        if let Some(id) = id {
            seen.insert(id.clone());
            let style = resolve_style(arena, node, theme, &self.styles, &self.state, screen_class);
            update_transition_amount(
                &mut self.state.hover_transition,
                &id,
                self.state.hovered.contains(&id),
                delta_time,
                style.transition_duration,
            );
            update_transition_amount(
                &mut self.state.pressed_transition,
                &id,
                self.state.pressed.as_ref() == Some(&id),
                delta_time,
                style.transition_duration,
            );
        }

        for child in children {
            self.update_node_transitions(arena, child, theme, screen_class, delta_time, seen);
        }
    }
}

fn update_transition_amount(
    transitions: &mut HashMap<ElementId, f32>,
    id: &ElementId,
    active: bool,
    delta_time: f32,
    duration: f32,
) {
    if duration <= TRANSITION_EPSILON {
        transitions.remove(id);
        return;
    }

    let current = transitions.get(id).copied().unwrap_or(0.0);
    let step = delta_time / duration;
    let next = if active {
        (current + step).min(1.0)
    } else {
        (current - step).max(0.0)
    };
    if next > 0.0 {
        transitions.insert(id.clone(), next);
    } else {
        transitions.remove(id);
    }
}

fn dispatch_path(
    layout: &LayoutNode,
    path: &[ElementId],
    kind: PointerEventKind,
    events: &mut Vec<UiEventRecord>,
) {
    for id in path.iter().take(path.len().saturating_sub(1)) {
        events.push(UiEventRecord {
            element_id: id.clone(),
            kind,
            phase: EventPhase::Capture,
        });
        if node_for_id(layout, id).is_some_and(|node| node.stop_propagation.contains(&kind)) {
            return;
        }
    }
    if let Some(target) = path.last() {
        events.push(UiEventRecord {
            element_id: target.clone(),
            kind,
            phase: EventPhase::Target,
        });
        if node_for_id(layout, target).is_some_and(|node| node.stop_propagation.contains(&kind)) {
            return;
        }
    }
    for id in path.iter().take(path.len().saturating_sub(1)).rev() {
        events.push(UiEventRecord {
            element_id: id.clone(),
            kind,
            phase: EventPhase::Bubble,
        });
        if node_for_id(layout, id).is_some_and(|node| node.stop_propagation.contains(&kind)) {
            return;
        }
    }
}

fn node_for_id<'a>(node: &'a LayoutNode, id: &ElementId) -> Option<&'a LayoutNode> {
    node.find(id)
}

fn path_to_id(root: &LayoutNode, target: &ElementId) -> Option<Vec<ElementId>> {
    let mut path = Vec::new();
    if collect_path(root, target, &mut path) {
        Some(path)
    } else {
        None
    }
}

fn collect_path(node: &LayoutNode, target: &ElementId, path: &mut Vec<ElementId>) -> bool {
    if let Some(id) = &node.id {
        path.push(id.clone());
        if id == target {
            return true;
        }
    }
    for child in &node.children {
        if collect_path(child, target, path) {
            return true;
        }
    }
    if node.id.is_some() {
        path.pop();
    }
    false
}

#[allow(dead_code)]
fn ids_in_tree(root: &LayoutNode) -> HashSet<ElementId> {
    let mut ids = HashSet::new();
    collect_ids(root, &mut ids);
    ids
}

fn collect_ids(node: &LayoutNode, ids: &mut HashSet<ElementId>) {
    if let Some(id) = &node.id {
        ids.insert(id.clone());
    }
    for child in &node.children {
        collect_ids(child, ids);
    }
}
