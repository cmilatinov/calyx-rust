use std::any::TypeId;
use engine::egui;
use engine::egui::Ui;
use reflect::{Reflect, ReflectGenericInt};
use reflect::registry::TypeRegistry;
use reflect::ReflectDefault;
use utils::type_ids;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct IntegerInspector;

impl TypeInspector for IntegerInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(
            u8, u16, u32, u64, u128, usize,
            i8, i16, i32, i64, i128, isize
        )
    }

    fn show_inspector(
        &self,
        ui: &mut Ui,
        registry: &TypeRegistry,
        instance: &mut dyn Reflect
    ) {
        let id = instance.as_any().type_id();
        let meta = registry.trait_meta::<ReflectGenericInt>(id).unwrap();
        let mut integer = meta.get(instance).unwrap().as_i64();
        ui.add(egui::Slider::new(&mut integer, 0..=360));
    }
}