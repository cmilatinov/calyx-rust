use engine::egui::{TextEdit, Ui};
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::{Reflect, ReflectDefault, TypeUuid};

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
            TextEdit::singleline(&mut str)
                .desired_width(ui.available_width())
                .show(ui);
        }
    }
}
