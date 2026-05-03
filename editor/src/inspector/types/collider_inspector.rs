use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use egui::{Id, Ui};
use engine::component::{ColliderShape, ComponentCollider, Orientation};
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use nalgebra_glm as glm;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct ColliderInspector;

impl ColliderInspector {
    fn collider_shape(ui: &mut Ui, ctx: &InspectorContext, value: &mut ColliderShape) -> bool {
        let mut changed = false;
        Widgets::inspector_prop_value_children(ui, "Shape", |phase| match phase {
            PropChildrenPhase::Value { ui, .. } => {
                let id = Id::new(ctx.game_object.node).with(ComponentCollider::type_uuid());
                egui::ComboBox::from_id_salt(id)
                    .selected_text(match value {
                        ColliderShape::Sphere { .. } => "Sphere",
                        ColliderShape::Capsule { .. } => "Capsule",
                        ColliderShape::Cuboid { .. } => "Cuboid",
                        ColliderShape::Cone { .. } => "Cone",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(
                                value,
                                ColliderShape::Sphere { radius: 1.0 },
                                "Sphere",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                value,
                                ColliderShape::Capsule {
                                    orientation: Orientation::Y,
                                    height: 1.0,
                                    radius: 1.0,
                                },
                                "Capsule",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                value,
                                ColliderShape::Cone {
                                    height: 1.0,
                                    radius: 1.0,
                                },
                                "Cone",
                            )
                            .changed();
                        changed |= ui
                            .selectable_value(
                                value,
                                ColliderShape::Cuboid {
                                    half_extents: glm::vec3(1.0, 1.0, 1.0),
                                },
                                "Cuboid",
                            )
                            .changed();
                    });
            }
            PropChildrenPhase::Children { ui } => match value {
                ColliderShape::Sphere { radius } => {
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                    });
                }
                ColliderShape::Capsule {
                    orientation,
                    height,
                    radius,
                } => {
                    Widgets::inspector_prop_value(ui, "Orientation", |ui, _| {
                        egui::ComboBox::from_id_salt("orientation")
                            .selected_text(match orientation {
                                Orientation::X => "X",
                                Orientation::Y => "Y",
                                Orientation::Z => "Z",
                            })
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(orientation, Orientation::X, "X")
                                    .changed();
                                changed |= ui
                                    .selectable_value(orientation, Orientation::Y, "Y")
                                    .changed();
                                changed |= ui
                                    .selectable_value(orientation, Orientation::Z, "Z")
                                    .changed();
                            });
                    });
                    Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                    });
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                    });
                }
                ColliderShape::Cuboid { half_extents } => {
                    Widgets::inspector_prop_value(ui, "Half Extents", |ui, _| {
                        changed |= Widgets::drag_float3(ui, 0.1, half_extents);
                    });
                }
                ColliderShape::Cone { height, radius } => {
                    Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                    });
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                    });
                }
            },
        });
        changed
    }
}

impl TypeInspector for ColliderInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(ComponentCollider)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let Some(value) = instance.downcast_mut::<ComponentCollider>() else {
            return;
        };
        let mut changed = false;
        Widgets::inspector_prop_value(ui, "Enabled", |ui, _| {
            changed |= ui
                .add(egui::Checkbox::without_text(&mut value.enabled))
                .changed();
        });
        changed |= Self::collider_shape(ui, ctx, &mut value.shape);
        Widgets::inspector_prop_value(ui, "Friction", |ui, _| {
            changed |= ui.add(egui::DragValue::new(&mut value.friction)).changed();
        });
        Widgets::inspector_prop_value(ui, "Density", |ui, _| {
            changed |= ui.add(egui::DragValue::new(&mut value.density)).changed();
        });
        if changed {
            value.dirty = true;
        }
    }
}
