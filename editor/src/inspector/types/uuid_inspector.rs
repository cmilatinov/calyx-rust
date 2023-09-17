use std::any::TypeId;
use engine::egui::Ui;
use engine::uuid::Uuid;
use reflect::{Reflect, ReflectDefault};
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector, InspectorContext};

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