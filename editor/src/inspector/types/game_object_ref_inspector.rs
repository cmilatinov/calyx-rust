use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::GameObjectRef;
use engine::type_uuids;
use engine::utils::TypeUuid;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct GameObjectRefInspector;

impl TypeInspector for GameObjectRefInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(GameObjectRef)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<GameObjectRef>() {
            Widgets::game_object_select(ui, ctx.field_name, ctx.scene, value);
        }
    }
}
