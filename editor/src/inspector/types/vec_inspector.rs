use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use nalgebra_glm::{Vec2, Vec3, Vec4};
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct VecInspector;

impl TypeInspector for VecInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(Vec2, Vec3, Vec4)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(vec) = instance.downcast_mut::<Vec2>() {
            Widgets::drag_float2(ui, 0.1, vec);
        } else if let Some(vec) = instance.downcast_mut::<Vec3>() {
            Widgets::drag_float3(ui, 0.1, vec);
        } else if let Some(vec) = instance.downcast_mut::<Vec4>() {
            Widgets::drag_float4(ui, 0.1, vec);
        }
    }
}
