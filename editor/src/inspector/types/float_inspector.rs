use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::{Deref, RangeInclusive};

use engine::egui;
use engine::egui::Ui;
use engine::reflect;
use engine::reflect::{AttributeValue, ReflectDefault};
use engine::reflect::{Reflect, ReflectGenericFloat};
use utils::type_ids;

use crate::inspector::type_inspector::{InspectorContext, ReflectTypeInspector, TypeInspector};

#[derive(Default, Clone, Reflect)]
#[reflect(Default, TypeInspector)]
pub struct FloatInspector;

impl TypeInspector for FloatInspector {
    fn target_type_ids(&self) -> Vec<TypeId> {
        type_ids!(f32, f64)
    }

    fn show_inspector(&self, ui: &mut Ui, ctx: &InspectorContext, instance: &mut dyn Reflect) {
        let empty_attrs = HashMap::new();
        let attrs = match ctx.field_name {
            Some(field) => ctx
                .type_info
                .fields
                .get(field)
                .map(|f| &f.attrs)
                .unwrap_or(&empty_attrs),
            None => &ctx.type_info.attrs,
        };
        let angle_attr = attrs.get("angle").copied();
        let min_attr = attrs.get("min").copied();
        let max_attr = attrs.get("min").copied();
        let speed_attr = attrs.get("speed").copied();
        let type_id = instance.as_any().type_id();
        let float_type_id = TypeId::of::<f32>();
        let reflect_default_type_id = TypeId::of::<ReflectDefault>();
        println!("{:?} == {:?}", type_id, float_type_id);
        let meta = ctx.registry.trait_meta::<ReflectDefault>(type_id).unwrap();
        meta.default();
        // if let Some(num) = meta.get_mut(instance) {
        //     let mut value = num.as_f64();
        //     if angle_attr.is_some() {
        //         value = value.to_degrees();
        //     }
        //     let mut drag = egui::DragValue::new(&mut value);
        //     let mut min = f64::NEG_INFINITY;
        //     let mut max = f64::INFINITY;
        //     if let Some(AttributeValue::Float(value)) = min_attr {
        //         min = value;
        //     }
        //     if let Some(AttributeValue::Float(value)) = max_attr {
        //         max = value;
        //     }
        //     drag = drag.clamp_range(RangeInclusive::new(min, max));
        //     if let Some(AttributeValue::Float(value)) = speed_attr {
        //         drag = drag.speed(value);
        //     }
        //     if angle_attr.is_some() {
        //         drag = drag.suffix("°");
        //     }
        //     let res = ui.add(drag);
        //     if res.changed() {
        //         if angle_attr.is_some() {
        //             value = value.to_radians();
        //         }
        //         num.set_from_f64(value);
        //     }
        // }
    }
}
