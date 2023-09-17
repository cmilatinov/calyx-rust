use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use engine::egui::Ui;
use engine::uuid::Uuid;
use reflect::{Reflect, ReflectDefault};
use std::any::TypeId;
use utils::type_ids;

#[derive(Default, Reflect)]
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
            ui.text_edit_singleline(&mut str);
        }
    }
}
