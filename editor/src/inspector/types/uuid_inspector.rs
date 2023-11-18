use std::any::TypeId;

use engine::egui::{TextEdit, Ui};
use engine::reflect;
use engine::reflect::{Reflect, ReflectDefault};
use engine::uuid::Uuid;
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct UuidInspector;

impl TypeInspector for UuidInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Uuid)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(uuid) = instance.downcast_mut::<Uuid>() {
            let value = uuid.to_string();
            let mut str = value.as_str();
            TextEdit::singleline(&mut str)
                .desired_width(ui.available_width())
                .show(ui);
        }
    }
}
