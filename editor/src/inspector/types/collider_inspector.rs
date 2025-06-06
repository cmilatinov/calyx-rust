use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::component::{ColliderShape, ComponentCollider, Orientation};
use engine::core::Ref;
use engine::reflect::{Reflect, ReflectDefault};
use engine::{type_uuids, TypeUuid};
use nalgebra_glm as glm;
use std::ops::DerefMut;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColliderInspector;

impl ColliderInspector {
    fn collider_shape(ui: &mut Ui, ctx: &InspectorContext, value: &mut ColliderShape) -> bool {
        struct State<'a> {
            changed: bool,
            value: &'a mut ColliderShape,
        }

        let ref1 = Ref::new(State {
            value,
            changed: false,
        });
        let ref2 = ref1.clone();
        let ref3 = ref1.clone();
        Widgets::inspector_prop_value_children(
            ui,
            "Shape",
            move |ui, _| {
                let mut state = ref1.write();
                egui::ComboBox::from_id_salt(ctx.game_object.node)
                    .selected_text(match state.value {
                        ColliderShape::Sphere { .. } => "Sphere",
                        ColliderShape::Capsule { .. } => "Capsule",
                        ColliderShape::Cuboid { .. } => "Cuboid",
                        ColliderShape::Cone { .. } => "Cone",
                    })
                    .show_ui(ui, |ui| {
                        state.changed |= ui
                            .selectable_value(
                                state.value,
                                ColliderShape::Sphere { radius: 1.0 },
                                "Sphere",
                            )
                            .changed();
                        state.changed |= ui
                            .selectable_value(
                                state.value,
                                ColliderShape::Capsule {
                                    orientation: Orientation::Y,
                                    height: 1.0,
                                    radius: 1.0,
                                },
                                "Capsule",
                            )
                            .changed();
                        state.changed |= ui
                            .selectable_value(
                                state.value,
                                ColliderShape::Cone {
                                    height: 1.0,
                                    radius: 1.0,
                                },
                                "Cone",
                            )
                            .changed();
                        state.changed |= ui
                            .selectable_value(
                                state.value,
                                ColliderShape::Cuboid {
                                    half_extents: glm::vec3(1.0, 1.0, 1.0),
                                },
                                "Cuboid",
                            )
                            .changed();
                    });
            },
            |ui| {
                let mut state_binding = ref2.write();
                let state = state_binding.deref_mut();
                match &mut state.value {
                    ColliderShape::Sphere { radius } => {
                        Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                            state.changed |=
                                ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
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
                                    state.changed |= ui
                                        .selectable_value(orientation, Orientation::X, "X")
                                        .changed();
                                    state.changed |= ui
                                        .selectable_value(orientation, Orientation::Y, "Y")
                                        .changed();
                                    state.changed |= ui
                                        .selectable_value(orientation, Orientation::Z, "Z")
                                        .changed();
                                });
                        });
                        Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                            state.changed |=
                                ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                        });
                        Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                            state.changed |=
                                ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                        });
                    }
                    ColliderShape::Cuboid { half_extents } => {
                        Widgets::inspector_prop_value(ui, "Half Extents", |ui, _| {
                            state.changed |= Widgets::drag_float3(ui, 0.1, half_extents);
                        });
                    }
                    ColliderShape::Cone { height, radius } => {
                        Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                            state.changed |=
                                ui.add(egui::DragValue::new(height).speed(0.1)).changed();
                        });
                        Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                            state.changed |=
                                ui.add(egui::DragValue::new(radius).speed(0.1)).changed();
                        });
                    }
                };
            },
        );
        let changed = ref3.read().changed;
        changed
    }
}

impl TypeInspector for ColliderInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(ComponentCollider)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<ComponentCollider>() {
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
}
