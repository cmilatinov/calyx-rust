use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use engine::component::ComponentRigidBody;
use engine::egui::Ui;
use engine::rapier3d::dynamics::RigidBodyType;
use engine::reflect::{Reflect, ReflectDefault};
use engine::{egui, type_ids, TypeUuid};
use std::any::TypeId;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct RigidBodyInspector;

impl RigidBodyInspector {
    fn rigid_body_type_label(ty: RigidBodyType) -> &'static str {
        match ty {
            RigidBodyType::Dynamic => "Dynamic",
            RigidBodyType::Fixed => "Fixed",
            RigidBodyType::KinematicPositionBased => "KinematicPositionBased",
            RigidBodyType::KinematicVelocityBased => "KinematicVelocityBased",
        }
    }
}

impl TypeInspector for RigidBodyInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(ComponentRigidBody)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<ComponentRigidBody>() {
            let mut changed = false;
            Widgets::inspector_prop_table(ui, |mut body| {
                Widgets::inspector_row(&mut body, "Enabled ", |ui| {
                    changed |= ui
                        .add(egui::Checkbox::without_text(&mut value.enabled))
                        .changed();
                });
                Widgets::inspector_row(&mut body, "Type ", |ui| {
                    egui::ComboBox::from_id_source(ctx.game_object.node)
                        .selected_text(Self::rigid_body_type_label(value.ty))
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut value.ty, RigidBodyType::Dynamic, "Dynamic")
                                .changed();
                            changed |= ui
                                .selectable_value(&mut value.ty, RigidBodyType::Fixed, "Fixed")
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut value.ty,
                                    RigidBodyType::KinematicPositionBased,
                                    "KinematicPositionBased",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut value.ty,
                                    RigidBodyType::KinematicVelocityBased,
                                    "KinematicVelocityBased",
                                )
                                .changed();
                        });
                });
                Widgets::inspector_row(&mut body, "Mass ", |ui| {
                    changed |= ui
                        .add(egui::DragValue::new(&mut value.mass).speed(0.1))
                        .changed();
                });
                Widgets::inspector_row(&mut body, "Gravity Scale ", |ui| {
                    changed |= ui
                        .add(egui::DragValue::new(&mut value.gravity_scale).speed(0.01))
                        .changed();
                });
                Widgets::inspector_row(&mut body, "Can Sleep ", |ui| {
                    changed |= ui
                        .add(egui::Checkbox::without_text(&mut value.can_sleep))
                        .changed();
                });
            });
            if changed {
                value.dirty = true;
            }
        }
    }
}