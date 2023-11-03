use reflect_derive::impl_reflect_value;
use std::any::Any;

use crate as reflect;
use crate::{Reflect, ReflectedType};

use super::std_float::ReflectGenericFloat;
use super::std_int::ReflectGenericInt;

impl_reflect_value!(u8(GenericInt));
impl_reflect_value!(u16(GenericInt));
impl_reflect_value!(u32(GenericInt));
impl_reflect_value!(u64(GenericInt));
impl_reflect_value!(u128(GenericInt));

impl_reflect_value!(i8(GenericInt));
impl_reflect_value!(i16(GenericInt));
impl_reflect_value!(i32(GenericInt));
impl_reflect_value!(i64(GenericInt));
impl_reflect_value!(i128(GenericInt));

impl_reflect_value!(f32(GenericFloat));
impl_reflect_value!(f64(GenericFloat));

impl_reflect_value!(String());

impl<T: Reflect + ReflectedType> Reflect for Option<T> {
    fn type_name(&self) -> &'static str {
        if let Some(inner) = self.as_ref() {
            inner.type_name()
        } else {
            "None"
        }
    }

    fn type_name_short(&self) -> &'static str {
        if let Some(inner) = self.as_ref() {
            inner.type_name_short()
        } else {
            "None"
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_reflect(&self) -> &dyn Reflect {
        self
    }

    fn as_reflect_mut(&mut self) -> &mut dyn Reflect {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}
