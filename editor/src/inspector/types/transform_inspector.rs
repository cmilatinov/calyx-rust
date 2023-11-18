use std::any::TypeId;

use crate::BASE_FONT_SIZE;
use engine::component::ComponentTransform;
use engine::egui::{Align, Layout, Ui};
use engine::egui_extras;
use engine::egui_extras::Column;
use engine::math::Transform;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Clone, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct TransformInspector;

impl TypeInspector for TransformInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(ComponentTransform)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(t_comp) = instance.downcast_mut::<ComponentTransform>() {
            egui_extras::TableBuilder::new(ui)
                .column(Column::auto().clip(true).resizable(true))
                .column(Column::remainder().clip(true))
                .cell_layout(Layout::left_to_right(Align::Center))
                .body(|mut body| {
                    let mut changed = false;
                    let parent_transform = ctx
                        .parent_node
                        .map(|parent| ctx.scene.get_world_transform(parent))
                        .unwrap_or(Transform::default());
                    let mut transform = ctx.scene.get_world_transform(ctx.node);
                    body.row(BASE_FONT_SIZE + 6.0, |mut row| {
                        row.col(|ui| {
                            ui.label("Position ");
                        });
                        row.col(|ui| {
                            changed |= Widgets::drag_float3(ui, 0.1, &mut transform.position);
                        });
                    });
                    body.row(BASE_FONT_SIZE + 6.0, |mut row| {
                        row.col(|ui| {
                            ui.label("Rotation ");
                        });
                        row.col(|ui| {
                            changed |= Widgets::drag_angle3(ui, &mut transform.rotation);
                        });
                    });
                    body.row(BASE_FONT_SIZE + 6.0, |mut row| {
                        row.col(|ui| {
                            ui.label("Scale ");
                        });
                        row.col(|ui| {
                            changed |= Widgets::drag_float3(ui, 0.1, &mut transform.scale);
                        });
                    });
                    if changed {
                        transform.update_matrix();
                        t_comp.transform.set_local_matrix(
                            &(parent_transform.inverse_matrix * transform.matrix),
                        );
                    }
                });
            t_comp.transform.update_matrix();
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
                    .parent_node
                    .map(|parent| ctx.scene.get_world_transform(parent))
                    .unwrap_or(Transform::default());
                t_comp
                    .transform
                    .set_local_matrix(&parent_transform.inverse_matrix);
                ui.close_menu()
            }
        }
    }
}

impl TransformInspector {}
