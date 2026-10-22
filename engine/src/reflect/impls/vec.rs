use std::any::Any;

use uuid::Uuid;

use crate::core::wrapper_type_uuid;
use crate::reflect::type_registry::TypeRegistry;
use crate::reflect::{Reflect, ReflectList, TypeName};
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

    fn as_list(&self) -> Option<&dyn ReflectList> {
        Some(self)
    }

    fn as_list_mut(&mut self) -> Option<&mut dyn ReflectList> {
        Some(self)
    }

    fn register_field_type(registry: &mut TypeRegistry) {
        registry.meta_field_list::<T>();
        T::register_field_type(registry);
    }
}

impl<T: Reflect + TypeUuid> ReflectList for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn element(&self, index: usize) -> Option<&dyn Reflect> {
        self.get(index).map(|element| element as &dyn Reflect)
    }

    fn element_mut(&mut self, index: usize) -> Option<&mut dyn Reflect> {
        self.get_mut(index)
            .map(|element| element as &mut dyn Reflect)
    }

    fn push(&mut self, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>> {
        let value = value.downcast::<T>()?;
        Vec::push(self, *value);
        Ok(())
    }

    fn remove(&mut self, index: usize) -> Option<Box<dyn Reflect>> {
        (index < Vec::len(self)).then(|| Box::new(Vec::remove(self, index)) as Box<dyn Reflect>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_type_uuid_depends_on_element_type() {
        assert_ne!(Vec::<f32>::type_uuid(), Vec::<u32>::type_uuid());
        assert_ne!(Vec::<f32>::type_uuid(), f32::type_uuid());
        assert_ne!(Vec::<Vec<f32>>::type_uuid(), Vec::<f32>::type_uuid());
        assert_eq!(Vec::<f32>::type_uuid(), Vec::<f32>::type_uuid());
    }

    #[test]
    fn vec_type_uuid_is_stable() {
        // Saved data and plugins identify list types by this UUID, so it must
        // not drift between builds.
        assert_eq!(
            Vec::<f32>::type_uuid(),
            Uuid::parse_str("c6542891-c3ae-d736-d490-838163fa1989").unwrap()
        );
    }

    #[test]
    fn vec_list_edits_elements_through_reflection() {
        let mut values = vec![1.0f32, 2.0];
        let list = values
            .as_reflect_mut()
            .as_list_mut()
            .expect("Vec is a list");

        *list.element_mut(0).unwrap().downcast_mut::<f32>().unwrap() = 5.0;
        assert!(list.push(Box::new(3.0f32)).is_ok());
        assert!(list.push(Box::new(4u32)).is_err());
        let removed = list.remove(1).expect("index 1 exists");
        assert_eq!(*removed.downcast::<f32>().ok().unwrap(), 2.0);
        assert!(list.remove(5).is_none());
        assert!(list.element(2).is_none());

        assert_eq!(values, vec![5.0, 3.0]);
    }

    #[test]
    fn non_list_values_are_not_lists() {
        let mut value = 1.0f32;
        assert!(value.as_reflect().as_list().is_none());
        assert!(value.as_reflect_mut().as_list_mut().is_none());
    }
}
