use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::Widgets;
use calyx_engine::component::ComponentRigidBody;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::{Id, Ui};
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

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let mut changed = InspectorResponse::NONE;
        if let Some(value) = instance.downcast_mut::<ComponentRigidBody>() {
            Widgets::inspector_prop_value(ui, "Enabled", |ui, _| {
                changed |= ui.add(egui::Checkbox::without_text(&mut value.enabled));
            });
            Widgets::inspector_prop_value(ui, "Type", |ui, _| {
                let id = Id::new(ctx.game_object.node).with(ComponentRigidBody::type_uuid());
                egui::ComboBox::from_id_salt(id)
                    .selected_text(Self::rigid_body_type_label(value.ty))
                    .show_ui(ui, |ui| {
                        changed |=
                            ui.selectable_value(&mut value.ty, RigidBodyType::Dynamic, "Dynamic");
                        changed |=
                            ui.selectable_value(&mut value.ty, RigidBodyType::Fixed, "Fixed");
                        changed |= ui.selectable_value(
                            &mut value.ty,
                            RigidBodyType::KinematicPositionBased,
                            "KinematicPositionBased",
                        );
                        changed |= ui.selectable_value(
                            &mut value.ty,
                            RigidBodyType::KinematicVelocityBased,
                            "KinematicVelocityBased",
                        );
                    });
            });
            Widgets::inspector_prop_value(ui, "Mass", |ui, _| {
                changed |= ui.add(egui::DragValue::new(&mut value.mass).speed(0.1));
            });
            Widgets::inspector_prop_value(ui, "Gravity Scale", |ui, _| {
                changed |= ui.add(egui::DragValue::new(&mut value.gravity_scale).speed(0.01));
            });
            Widgets::inspector_prop_value(ui, "Can Sleep", |ui, _| {
                changed |= ui.add(egui::Checkbox::without_text(&mut value.can_sleep));
            });
        }
        changed
    }
}
