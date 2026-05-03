use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use convert_case::{Case, Casing};
use egui::{Id, Ui};
use engine::component::ParticleSpawnShape;
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use nalgebra_glm::Vec3;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "41339285-5516-4e83-ae27-8003f26f1922"]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct ParticleSpawnShapeInspector;

impl ParticleSpawnShapeInspector {
    fn spawn_shape(ui: &mut Ui, ctx: &InspectorContext, value: &mut ParticleSpawnShape) {
        let label = ctx
            .field_name
            .map(|name| name.from_case(Case::Snake).to_case(Case::Title))
            .unwrap_or_else(|| String::from("Particle Spawn Shape"));
        Widgets::inspector_prop_value_children(ui, label, |phase| match phase {
            PropChildrenPhase::Value { ui, .. } => {
                let id = Id::new(ctx.game_object.node).with(ctx.field_name);
                egui::ComboBox::from_id_salt(id)
                    .selected_text(match value {
                        ParticleSpawnShape::Sphere { .. } => "Sphere",
                        ParticleSpawnShape::Box { .. } => "Box",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            value,
                            ParticleSpawnShape::Sphere { radius: 0.5 },
                            "Sphere",
                        );
                        ui.selectable_value(
                            value,
                            ParticleSpawnShape::Box {
                                extents: Vec3::from_element(0.5),
                            },
                            "Box",
                        );
                    });
            }
            PropChildrenPhase::Children { ui } => match value {
                ParticleSpawnShape::Sphere { radius } => {
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        ui.add(
                            egui::DragValue::new(radius)
                                .speed(0.1)
                                .range(0.0..=f32::MAX),
                        );
                    });
                }
                ParticleSpawnShape::Box { extents } => {
                    Widgets::inspector_prop_value(ui, "Extents", |ui, _| {
                        Widgets::drag_float3(ui, 0.1, extents);
                    });
                }
            },
        });
    }
}

impl TypeInspector for ParticleSpawnShapeInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![ParticleSpawnShape::type_uuid()]
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let Some(shape) = instance.downcast_mut::<ParticleSpawnShape>() else {
            return;
        };
        Self::spawn_shape(ui, ctx, shape);
    }
}
