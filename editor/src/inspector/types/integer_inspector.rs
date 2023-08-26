use std::any::{Any, TypeId};
use engine::egui;
use engine::egui::Ui;
use reflect::{Reflect, ReflectGenericInt};
use reflect::registry::TypeRegistry;
use reflect::ReflectDefault;
use crate::inspector::type_inspector::{TypeInspector, ReflectTypeInspector};

#[derive(Default, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct IntegerInspector;

impl TypeInspector for IntegerInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        vec![
            TypeId::of::<u8>(),
            TypeId::of::<u16>(),
            TypeId::of::<u32>(),
            TypeId::of::<u64>(),
            TypeId::of::<u128>(),
            TypeId::of::<i8>(),
            TypeId::of::<i16>(),
            TypeId::of::<i32>(),
            TypeId::of::<i64>(),
            TypeId::of::<i128>()
        ]
    }

    fn show_inspector(
        &self,
        registry: &TypeRegistry,
        instance: &mut dyn Reflect,
        ui: &mut Ui
    ) {
        let id = instance.as_any().type_id();
        let meta = registry.trait_meta::<ReflectGenericInt>(id).unwrap();
        let mut integer = meta.get(instance).unwrap().as_i64();
        ui.add(egui::Slider::new(&mut integer, 0..=360));
    }
}