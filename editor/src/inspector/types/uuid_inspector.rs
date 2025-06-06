use egui::{TextEdit, Ui};
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use uuid::Uuid;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct UuidInspector;

impl TypeInspector for UuidInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(Uuid)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(uuid) = instance.downcast_mut::<Uuid>() {
            let value = uuid.to_string();
            let mut str = value.as_str();
            TextEdit::singleline(&mut str).desired_width(130.0).show(ui);
        }
    }
}
