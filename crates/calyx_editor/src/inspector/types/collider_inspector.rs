use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::{PropChildrenPhase, Widgets};
use calyx_engine::component::{ColliderShape, ComponentCollider, Orientation};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::{Id, PopupCloseBehavior, Ui};
use nalgebra_glm as glm;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColliderInspector;

/// How many collision groups a mask holds.
const GROUP_COUNT: u32 = u32::BITS;

/// A short summary of a group mask: `All`, `None`, or its groups numbered
/// from 1.
fn group_mask_text(mask: u32) -> String {
    match mask {
        u32::MAX => "All".to_owned(),
        0 => "None".to_owned(),
        _ => (0..GROUP_COUNT)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| (bit + 1).to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

impl ColliderInspector {
    /// A dropdown of the 32 collision groups, one checkbox each, with `All`
    /// and `None` shortcuts.
    fn group_mask(ui: &mut Ui, id: Id, mask: &mut u32) -> InspectorResponse {
        let mut changed = InspectorResponse::NONE;
        egui::ComboBox::from_id_salt(id)
            .selected_text(group_mask_text(*mask))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.horizontal(|ui| {
                    changed |= ui.selectable_value(mask, u32::MAX, "All");
                    changed |= ui.selectable_value(mask, 0, "None");
                });
                ui.separator();
                for bit in 0..GROUP_COUNT {
                    let mut member = *mask & (1 << bit) != 0;
                    let response = ui.checkbox(&mut member, format!("Group {}", bit + 1));
                    if response.changed() {
                        *mask ^= 1 << bit;
                    }
                    changed |= response;
                }
            });
        changed
    }

    fn collider_shape(
        ui: &mut Ui,
        ctx: &InspectorContext,
        value: &mut ColliderShape,
    ) -> InspectorResponse {
        let mut changed = InspectorResponse::NONE;
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
                        changed |= ui.selectable_value(
                            value,
                            ColliderShape::Sphere { radius: 1.0 },
                            "Sphere",
                        );
                        changed |= ui.selectable_value(
                            value,
                            ColliderShape::Capsule {
                                orientation: Orientation::Y,
                                height: 1.0,
                                radius: 1.0,
                            },
                            "Capsule",
                        );
                        changed |= ui.selectable_value(
                            value,
                            ColliderShape::Cone {
                                height: 1.0,
                                radius: 1.0,
                            },
                            "Cone",
                        );
                        changed |= ui.selectable_value(
                            value,
                            ColliderShape::Cuboid {
                                half_extents: glm::vec3(1.0, 1.0, 1.0),
                            },
                            "Cuboid",
                        );
                    });
            }
            PropChildrenPhase::Children { ui } => match value {
                ColliderShape::Sphere { radius } => {
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1));
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
                                changed |= ui.selectable_value(orientation, Orientation::X, "X");
                                changed |= ui.selectable_value(orientation, Orientation::Y, "Y");
                                changed |= ui.selectable_value(orientation, Orientation::Z, "Z");
                            });
                    });
                    Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(height).speed(0.1));
                    });
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1));
                    });
                }
                ColliderShape::Cuboid { half_extents } => {
                    Widgets::inspector_prop_value(ui, "Half Extents", |ui, _| {
                        changed |= Widgets::drag_float3(ui, 0.1, half_extents);
                    });
                }
                ColliderShape::Cone { height, radius } => {
                    Widgets::inspector_prop_value(ui, "Height", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(height).speed(0.1));
                    });
                    Widgets::inspector_prop_value(ui, "Radius", |ui, _| {
                        changed |= ui.add(egui::DragValue::new(radius).speed(0.1));
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

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(value) = instance.downcast_mut::<ComponentCollider>() else {
            return InspectorResponse::NONE;
        };
        let mut changed = InspectorResponse::NONE;
        changed |= Self::collider_shape(ui, ctx, &mut value.shape);
        Widgets::inspector_prop_value(ui, "Friction", |ui, _| {
            changed |= ui.add(egui::DragValue::new(&mut value.friction));
        });
        Widgets::inspector_prop_value(ui, "Density", |ui, _| {
            changed |= ui.add(egui::DragValue::new(&mut value.density));
        });
        let id = Id::new(ctx.game_object.node).with(ComponentCollider::type_uuid());
        Widgets::inspector_prop_value(ui, "Member Of Groups", |ui, _| {
            changed |= Self::group_mask(ui, id.with("memberships"), &mut value.groups.memberships);
        });
        Widgets::inspector_prop_value(ui, "Interacts With Groups", |ui, _| {
            changed |= Self::group_mask(ui, id.with("filter"), &mut value.groups.filter);
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::group_mask_text;

    #[test]
    fn group_mask_text_names_groups_from_one() {
        assert_eq!(group_mask_text(u32::MAX), "All");
        assert_eq!(group_mask_text(0), "None");
        assert_eq!(group_mask_text(0b101), "1, 3");
        assert_eq!(group_mask_text(1 << 31), "32");
    }
}
