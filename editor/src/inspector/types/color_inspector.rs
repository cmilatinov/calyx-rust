use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use egui::color_picker::Alpha;
use egui::{Color32, Ui};
use engine::reflect::{Reflect, ReflectDefault};
use engine::type_uuids;
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColorInspector;

impl TypeInspector for ColorInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(Color32)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(color) = instance.downcast_mut::<Color32>() {
            ui.horizontal(|ui| {
                egui::color_picker::color_edit_button_srgba(ui, color, Alpha::OnlyBlend);
            });
        }
    }
}
