use engine::egui::{TextEdit, Ui};
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::ReflectDefault;
use reflect::{Reflect, TypeUuid};

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(String)
    }
    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        TextEdit::singleline(instance.downcast_mut::<String>().unwrap())
            .desired_width(ui.available_width())
            .show(ui);
    }
}
