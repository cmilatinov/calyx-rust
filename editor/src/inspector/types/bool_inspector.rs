use std::any::TypeId;

use engine::egui::Ui;
use reflect;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct BoolInspector;

impl TypeInspector for BoolInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(bool)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<bool>() {
            ui.checkbox(value, "");
        }
    }
}
