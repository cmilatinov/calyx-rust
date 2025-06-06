use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use egui::{TextEdit, Ui};
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(String)
    }
    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<String>() {
            TextEdit::singleline(value).desired_width(130.0).show(ui);
        }
    }
}
