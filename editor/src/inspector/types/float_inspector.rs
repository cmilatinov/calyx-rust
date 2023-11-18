use std::any::TypeId;

use engine::egui;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct FloatInspector;

impl TypeInspector for FloatInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(f32, f64)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(num) = instance.downcast_mut::<f32>() {
            ui.horizontal_centered(|ui| {
                ui.add(egui::DragValue::new(num));
            });
        } else if let Some(num) = instance.downcast_mut::<f64>() {
            ui.horizontal_centered(|ui| {
                ui.add(egui::DragValue::new(num));
            });
        }
    }
}
