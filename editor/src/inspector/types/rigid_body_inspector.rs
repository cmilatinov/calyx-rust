use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::component::ComponentRigidBody;
use engine::reflect::{Reflect, ReflectDefault};
use engine::{type_uuids, TypeUuid};
use rapier3d::dynamics::RigidBodyType;
use uuid::Uuid;

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
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(ComponentRigidBody)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<ComponentRigidBody>() {
            let mut changed = false;
            Widgets::inspector_prop_value(ui, "Enabled", |ui, _| {
                changed |= ui
                    .add(egui::Checkbox::without_text(&mut value.enabled))
                    .changed();
            });
            Widgets::inspector_prop_value(ui, "Type", |ui, _| {
                egui::ComboBox::from_id_salt(ctx.game_object.node)
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
            Widgets::inspector_prop_value(ui, "Mass", |ui, _| {
                changed |= ui
                    .add(egui::DragValue::new(&mut value.mass).speed(0.1))
                    .changed();
            });
            Widgets::inspector_prop_value(ui, "Gravity Scale", |ui, _| {
                changed |= ui
                    .add(egui::DragValue::new(&mut value.gravity_scale).speed(0.01))
                    .changed();
            });
            Widgets::inspector_prop_value(ui, "Can Sleep", |ui, _| {
                changed |= ui
                    .add(egui::Checkbox::without_text(&mut value.can_sleep))
                    .changed();
            });
            if changed {
                value.dirty = true;
            }
        }
    }
}
