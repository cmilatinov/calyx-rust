use std::any::TypeId;

use engine::egui::{Color32, Ui};
use reflect::{Reflect, ReflectDefault};
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct ColorInspector;

impl TypeInspector for ColorInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Color32)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(color) = instance.downcast_mut::<Color32>() {
            let mut arr = color.to_array();
            if ui.color_edit_button_srgba_premultiplied(&mut arr).changed() {
                *color = Color32::from_rgba_premultiplied(arr[0], arr[1], arr[2], arr[3]);
            }
        }
    }
}
