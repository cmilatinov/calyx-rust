use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
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
