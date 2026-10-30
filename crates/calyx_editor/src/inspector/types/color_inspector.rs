use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::color_picker::Alpha;
use egui::{Color32, Ui};
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColorInspector;

impl TypeInspector for ColorInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(Color32)
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        _ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let Some(color) = instance.downcast_mut::<Color32>() else {
            return InspectorResponse::NONE;
        };
        ui.horizontal(|ui| {
            egui::color_picker::color_edit_button_srgba(ui, color, Alpha::OnlyBlend).into()
        })
        .inner
    }
}
