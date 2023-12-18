use engine::egui::Ui;
use engine::utils::type_uuids;
use engine::uuid::Uuid;
use reflect::ReflectDefault;
use reflect::{Reflect, TypeUuid};

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct BoolInspector;

impl TypeInspector for BoolInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(bool)
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<bool>() {
            ui.checkbox(value, "");
        }
    }
}
