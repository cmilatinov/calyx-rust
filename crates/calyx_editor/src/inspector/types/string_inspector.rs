use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::{TextEdit, Ui};
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct StringInspector;

impl TypeInspector for StringInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(String)
    }
    fn show_inspector(
        &self,
        ui: &mut Ui,
        _ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        match instance.downcast_mut::<String>() {
            Some(value) => TextEdit::singleline(value)
                .desired_width(130.0)
                .show(ui)
                .response
                .into(),
            None => InspectorResponse::NONE,
        }
    }
}
