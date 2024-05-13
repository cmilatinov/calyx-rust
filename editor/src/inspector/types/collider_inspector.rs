use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use engine::component::{ColliderShape, ComponentCollider, Orientation};
use engine::egui::Ui;
use engine::egui_extras::TableBody;
use engine::reflect::{Reflect, ReflectDefault};
use engine::{egui, glm, type_ids, TypeUuid};
use std::any::TypeId;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColliderInspector;

impl ColliderInspector {
    fn collider_shape(
        body: &mut TableBody,
        ctx: &InspectorContext,
        value: &mut ColliderShape,
    ) -> bool {
        let mut changed = false;
        Widgets::inspector_row(body, "Shape ", |ui| {
            egui::ComboBox::from_id_source(ctx.game_object.node)
                .selected_text(match value {
                    ColliderShape::Sphere { .. } => "Sphere",
                    ColliderShape::Capsule { .. } => "Capsule",
                    ColliderShape::Cuboid { .. } => "Cuboid",
                    ColliderShape::Cone { .. } => "Cone",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(value, ColliderShape::Sphere { radius: 1.0 }, "Sphere")
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
        });
        match value {
            ColliderShape::Sphere { radius } => {
                Widgets::inspector_row(body, "Radius ", |ui| {
                    changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                });
            }
            ColliderShape::Capsule {
                orientation,
                height,
                radius,
            } => {
                Widgets::inspector_row(body, "Orientation ", |ui| {
                    egui::ComboBox::from_id_source("orientation")
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
                Widgets::inspector_row(body, "Height ", |ui| {
                    changed |= ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                });
                Widgets::inspector_row(body, "Radius ", |ui| {
                    changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                });
            }
            ColliderShape::Cuboid { half_extents } => {
                Widgets::inspector_row(body, "Half Extents ", |ui| {
                    changed |= Widgets::drag_float3(ui, 0.1, half_extents);
                });
            }
            ColliderShape::Cone { height, radius } => {
                Widgets::inspector_row(body, "Height ", |ui| {
                    changed |= ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                });
                Widgets::inspector_row(body, "Radius ", |ui| {
                    changed |= ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                });
            }
        };
        changed
    }
}

impl TypeInspector for ColliderInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(ComponentCollider)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<ComponentCollider>() {
            let mut changed = false;
            Widgets::inspector_prop_table(ui, |mut body| {
                Widgets::inspector_row(&mut body, "Enabled ", |ui| {
                    changed |= ui
                        .add(egui::Checkbox::without_text(&mut value.enabled))
                        .changed();
                });
                changed |= Self::collider_shape(&mut body, ctx, &mut value.shape);
                Widgets::inspector_row(&mut body, "Friction ", |ui| {
                    changed |= ui.add(egui::DragValue::new(&mut value.friction)).changed();
                });
                Widgets::inspector_row(&mut body, "Density ", |ui| {
                    changed |= ui.add(egui::DragValue::new(&mut value.density)).changed();
                });
            });
            if changed {
                value.dirty = true;
            }
        }
    }
}
