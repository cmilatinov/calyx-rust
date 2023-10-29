use reflect_derive::impl_reflect_value;

use crate as reflect;

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
