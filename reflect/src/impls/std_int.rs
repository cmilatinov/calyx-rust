use crate as reflect;
use reflect_derive::reflect_trait;

#[reflect_trait]
pub trait GenericInt {
    fn as_u64(&self) -> u64;
    fn as_i64(&self) -> i64;
}

macro_rules! impl_generic_int {
    ($t:ty) => {
        impl GenericInt for $t {
            fn as_u64(&self) -> u64 {
                *self as u64
            }

            fn as_i64(&self) -> i64 {
                *self as i64
            }
        }
    }
}

impl_generic_int!(u8);
impl_generic_int!(u16);
impl_generic_int!(u32);
impl_generic_int!(u64);
impl_generic_int!(u128);

impl_generic_int!(i8);
impl_generic_int!(i16);
impl_generic_int!(i32);
impl_generic_int!(i64);
impl_generic_int!(i128);