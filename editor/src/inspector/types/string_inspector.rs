use engine::egui::{TextEdit, Ui};
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_ids;
use engine::utils::TypeUuid;
use std::any::TypeId;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(String)
    }
    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        TextEdit::singleline(instance.downcast_mut::<String>().unwrap())
            .desired_width(ui.available_width())
            .show(ui);
    }
}
