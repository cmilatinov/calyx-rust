use super::curve_inspector::{preview_rect, widget_edit, within_visible_width};
use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use crate::widgets::GradientEditor;
use calyx_engine::math::Gradient;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::utils::TypeUuid;
use convert_case::{Case, Casing};
use egui::Ui;
use uuid::Uuid;

/// Height of the gradient strip under a gradient field, in points.
const STRIP_HEIGHT: f32 = 20.0;

/// Shows a [`Gradient`] field with the [`GradientEditor`] widget, under a row
/// that sketches the gradient, and reports the widget's edits to the
/// inspector.
#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "4b9e2c71-0d3a-4f6e-8a15-c27d9e6b3f08"]
#[reflect(Default, TypeInspector)]
pub struct GradientInspector;

impl TypeInspector for GradientInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![Gradient::type_uuid()]
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(gradient) = instance.downcast_mut::<Gradient>() else {
            return InspectorResponse::NONE;
        };
        let id = ui.id().with(ctx.field_name).with("gradient");
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Gradient"));
        let mut response = InspectorResponse::NONE;
        Widgets::inspector_prop_value_children(ui, label, |phase| match phase {
            PropChildrenPhase::Value { ui, .. } => {
                let rect = preview_rect(ui).shrink2(egui::vec2(0.0, 3.0));
                GradientEditor::paint_preview(ui, rect, gradient);
            }
            PropChildrenPhase::Children { ui } => {
                let edit = within_visible_width(ui, |ui| {
                    ui.add(
                        GradientEditor::new(gradient)
                            .id_salt(id)
                            .height(STRIP_HEIGHT),
                    )
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
