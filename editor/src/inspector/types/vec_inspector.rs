use std::any::TypeId;

use engine::egui::Ui;
use reflect::{Reflect, ReflectDefault};
use utils::type_ids;

use crate::glm::{Vec2, Vec3, Vec4};
use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct VecInspector;

impl TypeInspector for VecInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Vec2, Vec3, Vec4)
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
