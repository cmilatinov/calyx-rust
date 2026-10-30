use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use crate::inspector::widgets::Widgets;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::scene::GameObjectRef;
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct GameObjectRefInspector;

impl TypeInspector for GameObjectRefInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(GameObjectRef)
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        match instance.downcast_mut::<GameObjectRef>() {
            Some(value) => Widgets::game_object_select(ui, ctx.field_name, ctx.scene, value).into(),
            None => InspectorResponse::NONE,
        }
    }
}
