use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::component::ComponentTransform;
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use nalgebra::UnitQuaternion;
use nalgebra_glm::Vec3;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct TransformInspector;

impl TypeInspector for TransformInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(ComponentTransform)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(t_comp) = instance.downcast_mut::<ComponentTransform>() {
            let mut changed = false;
            let mut transform = ctx.scene.get_world_transform(ctx.game_object);
            let parent_transform = ctx
                .parent
                .map(|parent| ctx.scene.get_world_transform(parent))
                .unwrap_or_default();
            Widgets::inspector_prop_value(ui, "Position", |ui, _| {
                changed |= Widgets::drag_float3(ui, 0.1, &mut transform.position);
            });
            Widgets::inspector_prop_value(ui, "Rotation", |ui, _| {
                let (z, y, x) = transform.rotation.euler_angles();
                let mut euler_rot = Vec3::new(x, y, z);
                if Widgets::drag_angle3(ui, &mut euler_rot) {
                    transform.rotation =
                        UnitQuaternion::from_euler_angles(euler_rot.z, euler_rot.y, euler_rot.x);
                    changed = true;
                }
            });
            Widgets::inspector_prop_value(ui, "Scale", |ui, _| {
                changed |= Widgets::drag_float3(ui, 0.1, &mut transform.scale);
            });
            if changed {
                transform.update_matrix();
                t_comp
                    .transform
                    .set_local_matrix(&(parent_transform.inverse_matrix * transform.matrix));
            }
        }
    }

    fn show_inspector_context(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) {
        if let Some(t_comp) = instance.downcast_mut::<ComponentTransform>() {
            if ui.button("Reset").clicked() {
                let parent_transform = ctx
                    .parent
                    .map(|parent| ctx.scene.get_world_transform(parent))
                    .unwrap_or_default();
                t_comp
                    .transform
                    .set_local_matrix(&parent_transform.inverse_matrix);
                ui.close_menu()
            }
        }
    }
}

impl TransformInspector {}
