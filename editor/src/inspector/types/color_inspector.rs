use engine::egui::color_picker::Alpha;
use engine::egui::{Color32, Ui};
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use engine::{egui, type_ids};
use std::any::TypeId;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColorInspector;

impl TypeInspector for ColorInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Color32)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(color) = instance.downcast_mut::<Color32>() {
            ui.horizontal(|ui| {
                egui::color_picker::color_edit_button_srgba(ui, color, Alpha::OnlyBlend);
            });
        }
    }
}
