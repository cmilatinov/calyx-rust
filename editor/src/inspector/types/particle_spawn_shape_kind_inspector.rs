use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use egui::{ComboBox, Ui};
use engine::component::ParticleSpawnShapeKind;
use engine::reflect::{Reflect, ReflectDefault};
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[uuid = "41339285-5516-4e83-ae27-8003f26f1922"]
#[reflect(Default, TypeInspector)]
#[repr(C)]
pub struct ParticleSpawnShapeKindInspector;

impl TypeInspector for ParticleSpawnShapeKindInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        vec![ParticleSpawnShapeKind::type_uuid()]
    }

    fn show_inspector(&self, ui: &mut Ui, _ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let Some(kind) = instance.downcast_mut::<ParticleSpawnShapeKind>() else {
            return;
        };
        ComboBox::from_id_salt(ui.next_auto_id())
            .selected_text(match kind {
                ParticleSpawnShapeKind::Point => "Point",
                ParticleSpawnShapeKind::Sphere => "Sphere",
                ParticleSpawnShapeKind::Box => "Box",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(kind, ParticleSpawnShapeKind::Point, "Point");
                ui.selectable_value(kind, ParticleSpawnShapeKind::Sphere, "Sphere");
                ui.selectable_value(kind, ParticleSpawnShapeKind::Box, "Box");
            });
    }
}
