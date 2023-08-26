use std::any::TypeId;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::registry::TypeRegistry;
use reflect::ReflectDefault;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct FloatInspector;

impl TypeInspector for FloatInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        vec![
            TypeId::of::<f32>(),
            TypeId::of::<f64>()
        ]
    }

    fn show_inspector(&self, registry: &TypeRegistry, instance: &mut dyn Reflect, ui: &mut Ui) {
        todo!()
    }
}