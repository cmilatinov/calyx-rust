use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use engine::egui;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use std::any::TypeId;
use utils::type_ids;

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct FloatInspector;

impl TypeInspector for FloatInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(f32, f64)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(num) = instance.downcast_mut::<f32>() {
            ui.add(egui::DragValue::new(num));
        } else if let Some(num) = instance.downcast_mut::<f64>() {
            ui.add(egui::DragValue::new(num));
        }
    }
}
