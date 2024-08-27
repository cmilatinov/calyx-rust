use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};
use crate::inspector::widgets::Widgets;
use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::GameObjectRef;
use engine::type_ids;
use engine::utils::TypeUuid;
use std::any::TypeId;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct GameObjectRefInspector;

impl TypeInspector for GameObjectRefInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(Option<GameObjectRef>, GameObjectRef)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        if let Some(value) = instance.downcast_mut::<Option<GameObjectRef>>() {
            Widgets::game_object_select(ui, ctx.field_name, ctx.scene, value);
        }
    }
}
