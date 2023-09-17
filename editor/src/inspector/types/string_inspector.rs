use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use engine::egui::Ui;
use reflect::Reflect;
use reflect::ReflectDefault;
use std::any::TypeId;
use utils::type_ids;

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(String)
    }
    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        ui.text_edit_singleline(instance.downcast_mut::<String>().unwrap());
    }
}
