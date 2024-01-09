use engine_derive::impl_reflect_value;

use crate as engine;

use super::std_default::ReflectDefault;
use super::std_float::ReflectGenericFloat;
use super::std_int::ReflectGenericInt;

impl_reflect_value!(bool(Default));

impl_reflect_value!(u8(Default, GenericInt));
impl_reflect_value!(u16(Default, GenericInt));
impl_reflect_value!(u32(Default, GenericInt));
impl_reflect_value!(u64(Default, GenericInt));
impl_reflect_value!(u128(Default, GenericInt));

impl_reflect_value!(i8(Default, GenericInt));
impl_reflect_value!(i16(Default, GenericInt));
impl_reflect_value!(i32(Default, GenericInt));
impl_reflect_value!(i64(Default, GenericInt));
impl_reflect_value!(i128(Default, GenericInt));

impl_reflect_value!(usize(Default, GenericInt));
impl_reflect_value!(isize(Default, GenericInt));

impl_reflect_value!(f32(Default, GenericFloat));
impl_reflect_value!(f64(Default, GenericFloat));

impl_reflect_value!(String(Default));
