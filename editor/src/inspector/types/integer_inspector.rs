use engine::egui::Ui;
use engine::reflect::{Reflect, ReflectDefault, ReflectGenericInt};
use engine::utils::TypeUuid;
use engine::{egui, type_ids};
use std::any::TypeId;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, TypeUuid, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct IntegerInspector;

impl TypeInspector for IntegerInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let meta = ctx
            .registry
            .trait_meta::<ReflectGenericInt>(instance.as_any().type_id())
            .unwrap();
        let integer = meta.get_mut(instance).unwrap();
        let mut value = integer.as_i64();
        let res = ui.add(egui::DragValue::new(&mut value));
        if res.changed() {
            integer.set_from_i64(value);
        }
    }
}
