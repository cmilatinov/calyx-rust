use std::any::TypeId;
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector, InspectorContext};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(String)
    }
    fn show_inspector(
        &self,
        ui: &mut Ui,
        _ctx: &InspectorContext,
        instance: &mut dyn Reflect
    ) {
        ui.text_edit_singleline(instance.downcast_mut::<String>().unwrap());
    }
}
