use engine::egui;
use engine::egui::color_picker::Alpha;
use engine::egui::{Color32, Ui};
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::{Reflect, ReflectDefault, TypeUuid};

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

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
