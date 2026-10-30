use std::ops::{BitOr, BitOrAssign};

use calyx_engine::context::ReadOnlyRegistryContext;
use calyx_engine::reflect::{Reflect, StructInfo};
use calyx_engine::reflect_trait;
use calyx_engine::scene::{GameObject, SceneView};
use egui::{Response, Ui};
use uuid::Uuid;

#[reflect_trait]
pub trait TypeInspector: Send + Sync {
    fn target_type_uuids(&self) -> Vec<Uuid>;
    /// Draws the editing widgets for `instance` and reports what they did to
    /// it this frame. Every change the widgets make to `instance` must be
    /// reported as `changed`.
    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse;
    /// Whether the inspector draws its own labelled rows for a field,
    /// as enum inspectors do, instead of a value in the row the component
    /// inspector lays out for it. An editor taller than one row, such as the
    /// curve editor, draws its own rows.
    fn draws_own_rows(&self) -> bool {
        false
    }
    /// Draws the entries this inspector adds to the component's context menu
    /// and reports any change they made, as [`TypeInspector::show_inspector`]
    /// does.
    fn show_inspector_context(
        &self,
        _ui: &mut Ui,
        _ctx: &InspectorContext,
        _instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        InspectorResponse::NONE
    }
}

/// What the widgets of an inspector did to the value they edit this frame.
///
/// `changed` means the value was written. `drag_started` and `drag_stopped`
/// bracket a continuous interaction: a pointer drag, or the time a text field
/// holds keyboard focus. The inspector records one undo step per interaction,
/// so a drag across many frames is undone at once.
#[must_use]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspectorResponse {
    pub changed: bool,
    pub drag_started: bool,
    pub drag_stopped: bool,
}

impl InspectorResponse {
    /// Nothing happened.
    pub const NONE: Self = Self {
        changed: false,
        drag_started: false,
        drag_stopped: false,
    };

    /// A discrete edit: the value was written outside any interaction.
    pub const CHANGED: Self = Self {
        changed: true,
        drag_started: false,
        drag_stopped: false,
    };

    /// Reports a widget's response: its change, its drag, and its keyboard
    /// focus, which brackets typing the way a drag brackets dragging.
    pub fn from_response(response: &Response) -> Self {
        Self {
            changed: response.changed(),
            drag_started: response.drag_started() || response.gained_focus(),
            drag_stopped: response.drag_stopped() || response.lost_focus(),
        }
    }

    /// `CHANGED` when `changed` is true, `NONE` otherwise.
    pub fn changed_if(changed: bool) -> Self {
        if changed {
            Self::CHANGED
        } else {
            Self::NONE
        }
    }

    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }
}

impl From<Response> for InspectorResponse {
    fn from(response: Response) -> Self {
        Self::from_response(&response)
    }
}

impl From<&Response> for InspectorResponse {
    fn from(response: &Response) -> Self {
        Self::from_response(response)
    }
}

impl BitOr for InspectorResponse {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self {
            changed: self.changed | rhs.changed,
            drag_started: self.drag_started | rhs.drag_started,
            drag_stopped: self.drag_stopped | rhs.drag_stopped,
        }
    }
}

impl BitOrAssign for InspectorResponse {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl BitOrAssign<Response> for InspectorResponse {
    fn bitor_assign(&mut self, rhs: Response) {
        *self = *self | Self::from_response(&rhs);
    }
}

#[derive(Copy, Clone)]
pub struct InspectorContext<'a> {
    pub assets: &'a ReadOnlyRegistryContext,
    pub scene: &'a SceneView<'a>,
    pub game_object: GameObject,
    pub parent: Option<GameObject>,
    pub type_info: &'a StructInfo,
    pub field_name: Option<&'static str>,
}
