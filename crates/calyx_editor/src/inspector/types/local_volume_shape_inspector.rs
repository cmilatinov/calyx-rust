use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use calyx_engine::component::LocalVolumeShape;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::utils::TypeUuid;
use convert_case::{Case, Casing};
use egui::{Id, Ui};
use nalgebra_glm::Vec3;
use uuid::Uuid;

/// Edits a local post-process volume's shape: a choice of box or sphere,
/// then its size.
#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "b6f1d0a4-7c2e-4f59-8a3d-5e9c1b2f7a60"]
#[reflect(Default, TypeInspector)]
pub struct LocalVolumeShapeInspector;

impl LocalVolumeShapeInspector {
    fn shape(
        ui: &mut Ui,
        ctx: &InspectorContext,
        value: &mut LocalVolumeShape,
    ) -> InspectorResponse {
        let mut response = InspectorResponse::NONE;
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Shape"));
        // Switching shape keeps its size: a box's largest half extent becomes
        // the sphere's radius, and a sphere's radius every half extent.
        let as_box = match *value {
            LocalVolumeShape::Box { .. } => *value,
            LocalVolumeShape::Sphere { radius } => LocalVolumeShape::Box {
                half_extents: Vec3::from_element(radius),
            },
        };
        let as_sphere = match *value {
            LocalVolumeShape::Box { half_extents } => LocalVolumeShape::Sphere {
                radius: half_extents.abs().max(),
            },
            LocalVolumeShape::Sphere { .. } => *value,
        };
        Widgets::inspector_prop_value_children(ui, label, |phase| match phase {
            PropChildrenPhase::Value { ui, .. } => {
                let id = Id::new(ctx.game_object.node).with(ctx.field_name);
                egui::ComboBox::from_id_salt(id)
                    .selected_text(match value {
                        LocalVolumeShape::Box { .. } => "Box",
                        LocalVolumeShape::Sphere { .. } => "Sphere",
                    })
                    .show_ui(ui, |ui| {
                        response |= ui.selectable_value(value, as_box, "Box");
                        response |= ui.selectable_value(value, as_sphere, "Sphere");
                    });
            }
            PropChildrenPhase::Children { ui } => match value {
                LocalVolumeShape::Box { half_extents } => {
                    Widgets::inspector_prop_value(ui, "Half Extents", |ui, _| {
                        response |= Widgets::drag_float3(ui, 0.1, half_extents);
                    });
                }
                LocalVolumeShape::Sphere { radius } => {
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        response |= ui.add(
                            egui::DragValue::new(radius)
                                .speed(0.1)
                                .range(0.0..=f32::MAX),
                        );
                    });
                }
            },
        });
        response
    }
}

impl TypeInspector for LocalVolumeShapeInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![LocalVolumeShape::type_uuid()]
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(shape) = instance.downcast_mut::<LocalVolumeShape>() else {
            return InspectorResponse::NONE;
        };
        Self::shape(ui, ctx, shape)
    }
}
