use crate::inspector::type_inspector::{
    InspectorContext, InspectorResponse, ReflectTypeInspector, TypeInspector,
};
use calyx_engine::reflect::{Reflect, ReflectDefault, ReflectGenericInt};
use calyx_engine::type_uuids;
use calyx_engine::utils::TypeUuid;
use egui::Ui;
use uuid::Uuid;

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct IntegerInspector;

impl TypeInspector for IntegerInspector {
    fn target_type_uuids(&self) -> Vec<Uuid> {
        type_uuids!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize)
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        ctx: &InspectorContext,
        instance: &mut dyn Reflect,
    ) -> InspectorResponse {
        let type_registry = ctx.assets.types.read();
        let Some(meta) = type_registry.trait_meta::<ReflectGenericInt>(instance.uuid()) else {
            return InspectorResponse::NONE;
        };
        let Some(integer) = meta.get_mut(instance) else {
            return InspectorResponse::NONE;
        };
        let mut value = integer.as_i64();
        let res = ui.add(egui::DragValue::new(&mut value));
        if res.changed() {
            integer.set_from_i64(value);
        }
        InspectorResponse::from_response(&res)
    }
}
