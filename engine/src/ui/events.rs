use super::geometry::UiPoint;
use super::widgets::ElementId;

/// Pointer event kind emitted by the UI runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerEventKind {
    PointerEnter,
    PointerLeave,
    PointerMove,
    PointerDown,
    PointerUp,
    Click,
    DragStart,
    Drag,
    DragEnd,
}

/// Event dispatch phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventPhase {
    Capture,
    Target,
    Bubble,
}

/// One dispatched event record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiEventRecord {
    pub element_id: ElementId,
    pub kind: PointerEventKind,
    pub phase: EventPhase,
}

/// Pointer input sample for one UI frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiInput {
    pub pointer_position: Option<UiPoint>,
    pub pointer_down: bool,
}

/// Per-element response for the current frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElementResponse {
    pub hovered: bool,
    pub pressed: bool,
    pub clicked: bool,
    pub dragged: bool,
}
