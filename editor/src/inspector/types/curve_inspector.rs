use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use crate::widgets::CurveEditor;
use convert_case::{Case, Casing};
use egui::{vec2, Rect, Sense, Ui};
use engine::math::Curve;
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use re_ui::UiExt;
use uuid::Uuid;

/// Room the curve editor leaves for its key handles past the graph's edge.
const HANDLE_MARGIN: f32 = 4.0;
/// Height of the curve graph under a curve field, in points.
const GRAPH_HEIGHT: f32 = 72.0;

/// Shows a [`Curve`] field with the [`CurveEditor`] widget, under a row that
/// sketches the curve, and reports the widget's edits to the inspector.
#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "fb42379b-3cb7-49fd-aedd-780a0fef6186"]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct CurveInspector;

/// The width left on the cursor's row before the inspector's right edge and
/// its scroll bar: list item bodies can reach past that edge.
///
/// The edge is the list's full span, which scrolls with the content. The
/// clip rectangle does not, so measuring from it would widen the editor as
/// the inspector scrolls sideways, and widen the inspector with it.
pub(crate) fn visible_width(ui: &Ui) -> f32 {
    let scroll = &ui.spacing().scroll;
    (ui.full_span().max
        - ui.cursor().min.x
        - scroll.bar_width
        - scroll.bar_outer_margin
        - HANDLE_MARGIN)
        .max(0.0)
}

/// A row-high rectangle for a read-only sketch in a field row's value,
/// allocated with no width.
///
/// A property row registers the width of its value as the width every row of
/// the list asks for, plus its own padding, so a sketch that filled the row
/// would widen the inspector past its panel.
pub(crate) fn preview_rect(ui: &mut Ui) -> Rect {
    let height = ui.spacing().interact_size.y;
    let width = ui.available_width().min(visible_width(ui)).max(0.0);
    let rect = Rect::from_min_size(ui.cursor().min, vec2(width, height));
    ui.allocate_exact_size(vec2(0.0, height), Sense::hover());
    rect
}

/// Lays `add_contents` out no wider than the inspector shows.
pub(crate) fn within_visible_width<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width().min(visible_width(ui));
    ui.scope(|ui| {
        ui.set_max_width(width);
        add_contents(ui)
    })
    .inner
}

/// The inspector's report of a widget's response: its change and drag, but
/// not the focus of fields in its popups, which can close while a field
/// holds focus and would then never report its end.
pub(crate) fn widget_edit(response: &egui::Response) -> InspectorResponse {
    InspectorResponse {
        changed: response.changed(),
        drag_started: response.drag_started(),
        drag_stopped: response.drag_stopped(),
    }
}

impl TypeInspector for CurveInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![Curve::type_uuid()]
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(curve) = instance.downcast_mut::<Curve>() else {
            return InspectorResponse::NONE;
        };
        let id = ui.id().with(ctx.field_name).with("curve");
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Curve"));
        let mut response = InspectorResponse::NONE;
        Widgets::inspector_prop_value_children(ui, label, |phase| match phase {
            PropChildrenPhase::Value { ui, .. } => {
                let rect = preview_rect(ui);
                CurveEditor::paint_preview(ui, rect, curve);
            }
            PropChildrenPhase::Children { ui } => {
                let edit = within_visible_width(ui, |ui| {
                    ui.add(CurveEditor::new(curve).id_salt(id).height(GRAPH_HEIGHT))
                });
                response |= widget_edit(&edit);
            }
        });
        response
    }

    fn draws_own_rows(&self) -> bool {
        true
    }
}
