use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct BoolInspector;

impl TypeInspector for BoolInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(bool)
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        _ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        match instance.downcast_mut::<bool>() {
            Some(value) => ui.checkbox(value, "").into(),
            None => InspectorResponse::NONE,
        }
    }
}
