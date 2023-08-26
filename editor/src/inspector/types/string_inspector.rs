use std::any::TypeId;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::registry::TypeRegistry;
use reflect::ReflectDefault;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        vec![TypeId::of::<String>()]
    }
    fn show_inspector(
        &self,
        _registry: &TypeRegistry,
        instance: &mut dyn Reflect,
        ui: &mut Ui
    ) {
        ui.text_edit_singleline(instance.downcast_mut::<String>().unwrap());
    }
}
