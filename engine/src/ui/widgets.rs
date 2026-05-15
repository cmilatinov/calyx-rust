use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::geometry::{EdgeInsets, UiPoint, UiSize};
use super::layout::LayoutNode;
use super::paint::PaintCommand;
use super::style::{
    AlignItems, Border, CornerRadius, JustifyContent, PointerEvents, ScreenClass, Style,
    StylePatch, UiColor, UiLength,
};

/// Stable UI element identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ElementId(String);

impl ElementId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ElementId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ElementId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Layout behavior requested by a widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiLayoutKind {
    #[default]
    Leaf,
    Row,
    Column,
    Stack,
    Center,
}

/// Trait-dispatched widget behavior. Plugins can implement this trait for custom widgets.
pub trait UiWidget: Send + Sync + 'static {
    fn layout_kind(&self) -> UiLayoutKind {
        UiLayoutKind::Leaf
    }

    fn default_size(&self, style: &Style, max: UiSize) -> UiSize;

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>);
}

/// Handle to a widget instance inside [`UiArena`] typed storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetHandle {
    store: usize,
    index: usize,
}

/// Handle to a UI node inside [`UiArena`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UiNodeHandle(usize);

/// Runtime UI node stored in [`UiArena`].
#[derive(Clone, Debug, PartialEq)]
pub struct UiNode {
    pub id: Option<ElementId>,
    pub widget: WidgetHandle,
    pub class: Option<String>,
    pub style: StylePatch,
    pub hover_style: StylePatch,
    pub pressed_style: StylePatch,
    pub responsive_styles: Vec<(ScreenClass, StylePatch)>,
    pub stop_propagation: Vec<super::events::PointerEventKind>,
    pub children: Vec<UiNodeHandle>,
}

impl UiNode {
    fn new(widget: WidgetHandle) -> Self {
        Self {
            id: None,
            widget,
            class: None,
            style: Default::default(),
            hover_style: Default::default(),
            pressed_style: Default::default(),
            responsive_styles: Vec::new(),
            stop_propagation: Vec::new(),
            children: Vec::new(),
        }
    }
}

trait WidgetStore {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn get(&self, index: usize) -> &dyn UiWidget;
    fn clear(&mut self);
}

struct TypedWidgetStore<T: UiWidget> {
    widgets: Vec<T>,
}

impl<T: UiWidget> Default for TypedWidgetStore<T> {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
        }
    }
}

impl<T: UiWidget> WidgetStore for TypedWidgetStore<T> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get(&self, index: usize) -> &dyn UiWidget {
        &self.widgets[index]
    }

    fn clear(&mut self) {
        self.widgets.clear();
    }
}

/// Frame arena for UI nodes and widget instances.
///
/// Widget instances are stored in typed vectors, so adding plugin widgets does not require a
/// closed engine enum and repeated frames reuse each widget vector's capacity.
#[derive(Default)]
pub struct UiArena {
    nodes: Vec<UiNode>,
    stores: Vec<Box<dyn WidgetStore>>,
    store_indices: HashMap<TypeId, usize>,
}

impl UiArena {
    pub fn clear(&mut self) {
        self.nodes.clear();
        for store in &mut self.stores {
            store.clear();
        }
    }

    pub fn alloc_widget<T: UiWidget>(&mut self, widget: T) -> WidgetHandle {
        let type_id = TypeId::of::<T>();
        let store = if let Some(store) = self.store_indices.get(&type_id) {
            *store
        } else {
            let store = self.stores.len();
            self.stores.push(Box::new(TypedWidgetStore::<T>::default()));
            self.store_indices.insert(type_id, store);
            store
        };
        let typed_store = self.stores[store]
            .as_any_mut()
            .downcast_mut::<TypedWidgetStore<T>>()
            .expect("widget store type mismatch");
        let index = typed_store.widgets.len();
        typed_store.widgets.push(widget);
        WidgetHandle { store, index }
    }

    pub fn alloc_node<T: UiWidget>(&mut self, widget: T) -> UiNodeHandle {
        let widget = self.alloc_widget(widget);
        let handle = UiNodeHandle(self.nodes.len());
        self.nodes.push(UiNode::new(widget));
        handle
    }

    pub fn node(&self, handle: UiNodeHandle) -> &UiNode {
        &self.nodes[handle.0]
    }

    pub fn node_mut(&mut self, handle: UiNodeHandle) -> &mut UiNode {
        &mut self.nodes[handle.0]
    }

    pub fn widget(&self, handle: WidgetHandle) -> &dyn UiWidget {
        self.stores[handle.store].get(handle.index)
    }

    pub fn container(&mut self) -> UiNodeHandle {
        self.alloc_node(ContainerWidget)
    }

    pub fn text(&mut self, value: impl Into<String>) -> UiNodeHandle {
        self.alloc_node(TextWidget { text: value.into() })
    }

    pub fn image(&mut self, texture: impl Into<String>) -> UiNodeHandle {
        self.alloc_node(ImageWidget {
            texture: texture.into(),
        })
    }

    pub fn row(&mut self) -> UiNodeHandle {
        self.alloc_node(RowWidget)
    }

    pub fn column(&mut self) -> UiNodeHandle {
        self.alloc_node(ColumnWidget)
    }

    pub fn stack(&mut self) -> UiNodeHandle {
        self.alloc_node(StackWidget)
    }

    pub fn button(&mut self, label: impl Into<String>) -> UiNodeHandle {
        self.alloc_node(ButtonWidget {
            label: label.into(),
        })
    }

    pub fn progress_bar(&mut self, value: f32, fill: UiColor) -> UiNodeHandle {
        self.alloc_node(ProgressBarWidget {
            value: value.clamp(0.0, 1.0),
            fill,
        })
    }

    pub fn spacer(&mut self) -> UiNodeHandle {
        self.alloc_node(SpacerWidget)
            .flex_grow(self, 1.0)
            .width(self, UiLength::Fill)
            .height(self, UiLength::Fill)
    }

    pub fn sized_box(&mut self) -> UiNodeHandle {
        self.alloc_node(SizedBoxWidget)
    }

    pub fn center(&mut self, child: UiNodeHandle) -> UiNodeHandle {
        self.alloc_node(CenterWidget).child(self, child)
    }

    pub fn crosshair(&mut self, color: UiColor, size: f32) -> UiNodeHandle {
        self.alloc_node(CrosshairWidget { color, size })
    }

    pub fn custom_paint(&mut self, commands: Vec<PaintCommand>) -> UiNodeHandle {
        self.alloc_node(CustomPaintWidget { commands })
    }
}

impl UiNodeHandle {
    pub fn id(self, arena: &mut UiArena, id: impl Into<ElementId>) -> Self {
        arena.node_mut(self).id = Some(id.into());
        self
    }

    pub fn class(self, arena: &mut UiArena, class: impl Into<String>) -> Self {
        arena.node_mut(self).class = Some(class.into());
        self
    }

    pub fn child(self, arena: &mut UiArena, child: UiNodeHandle) -> Self {
        arena.node_mut(self).children.push(child);
        self
    }

    pub fn children(
        self,
        arena: &mut UiArena,
        children: impl IntoIterator<Item = UiNodeHandle>,
    ) -> Self {
        arena.node_mut(self).children.extend(children);
        self
    }

    pub fn style(self, arena: &mut UiArena, patch: StylePatch) -> Self {
        arena.node_mut(self).style.merge_from(patch);
        self
    }

    pub fn hover_style(self, arena: &mut UiArena, patch: StylePatch) -> Self {
        arena.node_mut(self).hover_style = patch;
        self
    }

    pub fn pressed_style(self, arena: &mut UiArena, patch: StylePatch) -> Self {
        arena.node_mut(self).pressed_style = patch;
        self
    }

    pub fn when(self, arena: &mut UiArena, screen_class: ScreenClass, patch: StylePatch) -> Self {
        arena
            .node_mut(self)
            .responsive_styles
            .push((screen_class, patch));
        self
    }

    pub fn stop_propagation_on(
        self,
        arena: &mut UiArena,
        event: super::events::PointerEventKind,
    ) -> Self {
        arena.node_mut(self).stop_propagation.push(event);
        self
    }

    pub fn width(self, arena: &mut UiArena, value: UiLength) -> Self {
        self.style_patch(arena, |style| style.width = Some(value))
    }

    pub fn height(self, arena: &mut UiArena, value: UiLength) -> Self {
        self.style_patch(arena, |style| style.height = Some(value))
    }

    pub fn padding(self, arena: &mut UiArena, value: EdgeInsets) -> Self {
        self.style_patch(arena, |style| style.padding = Some(value))
    }

    pub fn margin(self, arena: &mut UiArena, value: EdgeInsets) -> Self {
        self.style_patch(arena, |style| style.margin = Some(value))
    }

    pub fn gap(self, arena: &mut UiArena, value: f32) -> Self {
        self.style_patch(arena, |style| style.gap = Some(value))
    }

    pub fn background(self, arena: &mut UiArena, value: UiColor) -> Self {
        self.style_patch(arena, |style| style.background = Some(value))
    }

    pub fn border(self, arena: &mut UiArena, value: Border) -> Self {
        self.style_patch(arena, |style| style.border = Some(value))
    }

    pub fn radius(self, arena: &mut UiArena, value: CornerRadius) -> Self {
        self.style_patch(arena, |style| style.radius = Some(value))
    }

    pub fn pointer_events(self, arena: &mut UiArena, value: PointerEvents) -> Self {
        self.style_patch(arena, |style| style.pointer_events = Some(value))
    }

    pub fn flex_grow(self, arena: &mut UiArena, value: f32) -> Self {
        self.style_patch(arena, |style| style.flex_grow = Some(value))
    }

    pub fn align_items(self, arena: &mut UiArena, value: AlignItems) -> Self {
        self.style_patch(arena, |style| style.align_items = Some(value))
    }

    pub fn justify_content(self, arena: &mut UiArena, value: JustifyContent) -> Self {
        self.style_patch(arena, |style| style.justify_content = Some(value))
    }

    pub fn transition_duration(self, arena: &mut UiArena, value: f32) -> Self {
        self.style_patch(arena, |style| style.transition_duration = Some(value))
    }

    fn style_patch(self, arena: &mut UiArena, update: impl FnOnce(&mut StylePatch)) -> Self {
        update(&mut arena.node_mut(self).style);
        self
    }
}

pub struct ContainerWidget;
pub struct RowWidget;
pub struct ColumnWidget;
pub struct StackWidget;
pub struct CenterWidget;
pub struct SpacerWidget;
pub struct SizedBoxWidget;

pub struct TextWidget {
    pub text: String,
}

pub struct ImageWidget {
    pub texture: String,
}

pub struct ButtonWidget {
    pub label: String,
}

pub struct ProgressBarWidget {
    pub value: f32,
    pub fill: UiColor,
}

pub struct CrosshairWidget {
    pub color: UiColor,
    pub size: f32,
}

pub struct CustomPaintWidget {
    pub commands: Vec<PaintCommand>,
}

impl UiWidget for ContainerWidget {
    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for RowWidget {
    fn layout_kind(&self) -> UiLayoutKind {
        UiLayoutKind::Row
    }

    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for ColumnWidget {
    fn layout_kind(&self) -> UiLayoutKind {
        UiLayoutKind::Column
    }

    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for StackWidget {
    fn layout_kind(&self) -> UiLayoutKind {
        UiLayoutKind::Stack
    }

    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for CenterWidget {
    fn layout_kind(&self) -> UiLayoutKind {
        UiLayoutKind::Center
    }

    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for SpacerWidget {
    fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
        UiSize::ZERO
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for SizedBoxWidget {
    fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
        UiSize::ZERO
    }

    fn paint(&self, _node: &LayoutNode, _commands: &mut Vec<PaintCommand>) {}
}

impl UiWidget for TextWidget {
    fn default_size(&self, style: &Style, _max: UiSize) -> UiSize {
        UiSize::new(
            self.text.len() as f32 * style.font_size * 0.5,
            style.font_size * 1.25,
        )
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        commands.push(PaintCommand::Text {
            rect: node.content_rect,
            text: self.text.clone(),
            color: node.style.text_color.with_alpha(node.style.opacity),
            font_size: node.style.font_size,
        });
    }
}

impl UiWidget for ButtonWidget {
    fn default_size(&self, style: &Style, _max: UiSize) -> UiSize {
        UiSize::new(
            self.label.len() as f32 * style.font_size * 0.55 + style.padding.horizontal(),
            style.font_size * 1.4 + style.padding.vertical(),
        )
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        commands.push(PaintCommand::Text {
            rect: node.content_rect,
            text: self.label.clone(),
            color: node.style.text_color.with_alpha(node.style.opacity),
            font_size: node.style.font_size,
        });
    }
}

impl UiWidget for ImageWidget {
    fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
        UiSize::ZERO
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        commands.push(PaintCommand::Image {
            rect: node.content_rect,
            texture: self.texture.clone(),
            tint: node.style.text_color.with_alpha(node.style.opacity),
            radius: node.style.radius,
        });
    }
}

impl UiWidget for ProgressBarWidget {
    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        UiSize::new(160.0_f32.min(max.width), 16.0_f32.min(max.height))
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        let fill_rect = super::geometry::UiRect {
            max: UiPoint::new(
                node.content_rect.min.x + node.content_rect.width() * self.value.clamp(0.0, 1.0),
                node.content_rect.max.y,
            ),
            ..node.content_rect
        };
        commands.push(PaintCommand::FillRect {
            rect: fill_rect,
            color: self.fill.with_alpha(node.style.opacity),
            radius: node.style.radius,
        });
    }
}

impl UiWidget for CrosshairWidget {
    fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
        UiSize::new(self.size, self.size)
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        let center = UiPoint::new(
            node.rect.min.x + node.rect.width() * 0.5,
            node.rect.min.y + node.rect.height() * 0.5,
        );
        let half = self.size * 0.5;
        commands.push(PaintCommand::Line {
            start: UiPoint::new(center.x - half, center.y),
            end: UiPoint::new(center.x + half, center.y),
            color: self.color,
            width: 1.0,
        });
        commands.push(PaintCommand::Line {
            start: UiPoint::new(center.x, center.y - half),
            end: UiPoint::new(center.x, center.y + half),
            color: self.color,
            width: 1.0,
        });
    }
}

impl UiWidget for CustomPaintWidget {
    fn default_size(&self, _style: &Style, _max: UiSize) -> UiSize {
        UiSize::ZERO
    }

    fn paint(&self, _node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        commands.extend(self.commands.iter().cloned());
    }
}
