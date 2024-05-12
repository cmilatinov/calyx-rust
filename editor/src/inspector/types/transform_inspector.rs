use engine::component::ComponentTransform;
use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_ids;
use engine::utils::TypeUuid;
use std::any::TypeId;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct TransformInspector;

impl TypeInspector for TransformInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(ComponentTransform)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(t_comp) = instance.downcast_mut::<ComponentTransform>() {
            Widgets::inspector_prop_table(ui, |mut body| {
                let mut changed = false;
                let mut transform = ctx.scene.get_world_transform(ctx.game_object);
                let parent_transform = ctx
                    .parent
                    .map(|parent| ctx.scene.get_world_transform(parent))
                    .unwrap_or_default();
                Widgets::inspector_row(&mut body, "Position ", |ui| {
                    changed |= Widgets::drag_float3(ui, 0.1, &mut transform.position);
                });
                Widgets::inspector_row(&mut body, "Rotation ", |ui| {
                    changed |= Widgets::drag_angle3(ui, &mut transform.rotation);
                });
                Widgets::inspector_row(&mut body, "Scale ", |ui| {
                    changed |= Widgets::drag_float3(ui, 0.1, &mut transform.scale);
                });
                if changed {
                    transform.update_matrix();
                    t_comp
                        .transform
                        .set_local_matrix(&(parent_transform.inverse_matrix * transform.matrix));
                }
            });
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
