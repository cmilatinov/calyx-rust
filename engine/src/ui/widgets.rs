use super::geometry::EdgeInsets;
use super::paint::PaintCommand;
use super::style::{
    AlignItems, Border, CornerRadius, JustifyContent, PointerEvents, ScreenClass, StylePatch,
    UiColor, UiLength,
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

/// Built-in widget kinds. Custom gameplay HUD widgets should compose these.
#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Container,
    Text { text: String },
    Image { texture: String },
    Row,
    Column,
    Stack,
    Button { label: String },
    ProgressBar { value: f32, fill: UiColor },
    Spacer,
    SizedBox,
    Center,
    Crosshair { color: UiColor, size: f32 },
    CustomPaint { commands: Vec<PaintCommand> },
}

/// Declarative UI node built by gameplay code each frame.
#[derive(Clone, Debug, PartialEq)]
pub struct UiNode {
    pub id: Option<ElementId>,
    pub kind: WidgetKind,
    pub class: Option<String>,
    pub style: StylePatch,
    pub hover_style: StylePatch,
    pub pressed_style: StylePatch,
    pub responsive_styles: Vec<(ScreenClass, StylePatch)>,
    pub stop_propagation: Vec<super::events::PointerEventKind>,
    pub children: Vec<UiNode>,
}

impl UiNode {
    pub fn new(kind: WidgetKind) -> Self {
        Self {
            id: None,
            kind,
            class: None,
            style: Default::default(),
            hover_style: Default::default(),
            pressed_style: Default::default(),
            responsive_styles: Vec::new(),
            stop_propagation: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = Some(class.into());
        self
    }

    pub fn child(mut self, child: UiNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = UiNode>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn style(mut self, patch: StylePatch) -> Self {
        self.style.merge_from(patch);
        self
    }

    pub fn hover_style(mut self, patch: StylePatch) -> Self {
        self.hover_style = patch;
        self
    }

    pub fn pressed_style(mut self, patch: StylePatch) -> Self {
        self.pressed_style = patch;
        self
    }

    pub fn when(mut self, screen_class: ScreenClass, patch: StylePatch) -> Self {
        self.responsive_styles.push((screen_class, patch));
        self
    }

    pub fn stop_propagation_on(mut self, event: super::events::PointerEventKind) -> Self {
        self.stop_propagation.push(event);
        self
    }

    pub fn width(self, value: UiLength) -> Self {
        self.style_patch(|style| style.width = Some(value))
    }

    pub fn height(self, value: UiLength) -> Self {
        self.style_patch(|style| style.height = Some(value))
    }

    pub fn padding(self, value: EdgeInsets) -> Self {
        self.style_patch(|style| style.padding = Some(value))
    }

    pub fn margin(self, value: EdgeInsets) -> Self {
        self.style_patch(|style| style.margin = Some(value))
    }

    pub fn gap(self, value: f32) -> Self {
        self.style_patch(|style| style.gap = Some(value))
    }

    pub fn background(self, value: UiColor) -> Self {
        self.style_patch(|style| style.background = Some(value))
    }

    pub fn border(self, value: Border) -> Self {
        self.style_patch(|style| style.border = Some(value))
    }

    pub fn radius(self, value: CornerRadius) -> Self {
        self.style_patch(|style| style.radius = Some(value))
    }

    pub fn pointer_events(self, value: PointerEvents) -> Self {
        self.style_patch(|style| style.pointer_events = Some(value))
    }

    pub fn flex_grow(self, value: f32) -> Self {
        self.style_patch(|style| style.flex_grow = Some(value))
    }

    pub fn align_items(self, value: AlignItems) -> Self {
        self.style_patch(|style| style.align_items = Some(value))
    }

    pub fn justify_content(self, value: JustifyContent) -> Self {
        self.style_patch(|style| style.justify_content = Some(value))
    }

    fn style_patch(mut self, update: impl FnOnce(&mut StylePatch)) -> Self {
        update(&mut self.style);
        self
    }
}

pub fn container() -> UiNode {
    UiNode::new(WidgetKind::Container)
}

pub fn text(value: impl Into<String>) -> UiNode {
    UiNode::new(WidgetKind::Text { text: value.into() })
}

pub fn image(texture: impl Into<String>) -> UiNode {
    UiNode::new(WidgetKind::Image {
        texture: texture.into(),
    })
}

pub fn row() -> UiNode {
    UiNode::new(WidgetKind::Row)
}

pub fn column() -> UiNode {
    UiNode::new(WidgetKind::Column)
}

pub fn stack() -> UiNode {
    UiNode::new(WidgetKind::Stack)
}

pub fn button(label: impl Into<String>) -> UiNode {
    UiNode::new(WidgetKind::Button {
        label: label.into(),
    })
}

pub fn progress_bar(value: f32, fill: UiColor) -> UiNode {
    UiNode::new(WidgetKind::ProgressBar {
        value: value.clamp(0.0, 1.0),
        fill,
    })
}

pub fn spacer() -> UiNode {
    UiNode::new(WidgetKind::Spacer)
        .flex_grow(1.0)
        .width(UiLength::Fill)
        .height(UiLength::Fill)
}

pub fn sized_box() -> UiNode {
    UiNode::new(WidgetKind::SizedBox)
}

pub fn center(child: UiNode) -> UiNode {
    UiNode::new(WidgetKind::Center).child(child)
}

pub fn crosshair(color: UiColor, size: f32) -> UiNode {
    UiNode::new(WidgetKind::Crosshair { color, size })
}

pub fn custom_paint(commands: Vec<PaintCommand>) -> UiNode {
    UiNode::new(WidgetKind::CustomPaint { commands })
}
