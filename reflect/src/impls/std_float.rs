use crate as reflect;
use reflect_derive::reflect_trait;

#[reflect_trait]
pub trait GenericFloat {
    fn as_f32(&self) -> f32;
    fn as_f64(&self) -> f64;
    fn set_from_f64(&mut self, value: f64);
    fn set_from_f32(&mut self, value: f32);
}

macro_rules! impl_generic_float {
    ($t:ty) => {
        impl GenericFloat for $t {
            fn as_f32(&self) -> f32 {
                *self as f32
            }
            fn as_f64(&self) -> f64 {
                *self as f64
            }
            fn set_from_f64(&mut self, value: f64) {
                *self = value as $t;
            }
            fn set_from_f32(&mut self, value: f32) {
                *self = value as $t;
            }
        }
    }
}

impl_generic_float!(f32);
impl_generic_float!(f64);