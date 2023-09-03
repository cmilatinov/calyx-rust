use std::any::TypeId;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::registry::TypeRegistry;
use reflect::ReflectDefault;
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct FloatInspector;

impl TypeInspector for FloatInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(f32, f64)
    }

    fn show_inspector(&self, _ui: &mut Ui, _registry: &TypeRegistry, _instance: &mut dyn Reflect) {
        todo!()
    }
}