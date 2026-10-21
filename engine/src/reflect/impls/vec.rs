use std::any::Any;

use uuid::Uuid;

use crate::core::wrapper_type_uuid;
use crate::reflect::{Reflect, TypeName};
use crate::utils::TypeUuid;

const VEC_WRAPPER_UUID: [u8; 16] = [
    0x5B, 0x8E, 0x2A, 0x41, 0x9C, 0x07, 0x4D, 0x3F, 0xA6, 0x1E, 0x72, 0xD4, 0x0B, 0x93, 0xC8, 0x56,
];

impl<T: TypeUuid> TypeUuid for Vec<T> {
    fn type_uuid() -> Uuid {
        wrapper_type_uuid(&VEC_WRAPPER_UUID, T::type_uuid())
    }
}

impl<T: Reflect + TypeUuid> TypeName for Vec<T> {
    fn type_name() -> &'static str {
        std::any::type_name::<Self>()
    }

    fn type_name_short() -> &'static str {
        "Vec"
    }
}

impl<T: Reflect + TypeUuid> Reflect for Vec<T> {
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

    fn assign(&mut self, value: Box<dyn Reflect>) -> bool {
        if let Ok(value) = value.downcast::<Self>() {
            *self = *value;
            true
        } else {
            false
        }
    }

    fn visit_elements_mut(&mut self, visit: &mut dyn FnMut(&mut dyn Reflect)) {
        for element in self.iter_mut() {
            visit(element);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_type_uuid_depends_on_element_type() {
        assert_ne!(Vec::<f32>::type_uuid(), Vec::<u32>::type_uuid());
        assert_ne!(Vec::<f32>::type_uuid(), f32::type_uuid());
        assert_eq!(Vec::<f32>::type_uuid(), Vec::<f32>::type_uuid());
    }

    #[test]
    fn vec_visits_each_element() {
        let mut values = vec![1.0f32, 2.0, 3.0];
        values.visit_elements_mut(&mut |element| {
            *element.downcast_mut::<f32>().unwrap() *= 10.0;
        });
        assert_eq!(values, vec![10.0, 20.0, 30.0]);
    }
}
