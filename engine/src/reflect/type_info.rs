use crate::reflect::Reflect;
use crate::utils::TypeUuid;
use std::any::TypeId;
use std::collections::HashMap;
use uuid::Uuid;

pub type FieldGetter = fn(&dyn Reflect) -> Option<&dyn Reflect>;
pub type FieldGetterMut = fn(&mut dyn Reflect) -> Option<&mut dyn Reflect>;
pub type FieldSetter = fn(&mut dyn Reflect, Box<dyn Reflect>) -> Option<()>;

#[derive(Copy, Clone)]
#[repr(C)]
pub enum AttributeValue {
    None,
    Float(f64),
    Integer(isize),
    String(&'static str),
}
pub type AttributeMap = HashMap<&'static str, AttributeValue>;

/// Allows you to introspect structs (and potentially other types in the future)
/// at runtime by listing, querying and setting their fields
#[repr(C)]
pub enum TypeInfo {
    Struct(StructInfo),
    None,
}

#[repr(C)]
pub struct StructInfo {
    pub type_name: &'static str,
    pub type_id: TypeId,
    pub attrs: AttributeMap,
    pub fields: HashMap<&'static str, NamedField>,
}

impl StructInfo {
    pub fn field(&self, name: &str) -> Option<&NamedField> {
        self.fields.get(name)
    }

    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

#[repr(C)]
pub struct NamedField {
    pub name: &'static str,
    pub type_id: TypeId,
    pub type_uuid: Uuid,
    pub type_name: &'static str,
    pub attrs: AttributeMap,
    pub doc: Option<&'static str>,
    pub getter: FieldGetter,
    pub getter_mut: FieldGetterMut,
    pub setter: FieldSetter,
}

impl NamedField {
    pub fn get<'a, T: 'static + Reflect + TypeUuid>(
        &'a self,
        instance: &'a dyn Reflect,
    ) -> Option<&'a T> {
        let value = (self.getter)(instance)?;
        value.downcast_ref::<T>()
    }
    pub fn get_mut<'a, T: 'static + Reflect + TypeUuid>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut T> {
        let value = (self.getter_mut)(instance)?;
        value.downcast_mut::<T>()
    }
    pub fn get_reflect<'a>(&'a self, instance: &'a dyn Reflect) -> Option<&'a dyn Reflect> {
        (self.getter)(instance)
    }
    pub fn get_reflect_mut<'a>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut dyn Reflect> {
        (self.getter_mut)(instance)
    }
    pub fn set<T: Reflect + 'static>(&self, instance: &mut dyn Reflect, value: T) -> Option<()> {
        (self.setter)(instance, Box::new(value))
    }
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Transform;
    use crate::reflect::type_registry::TypeRegistry;
    use crate::reflect::{Reflect, ReflectedType};
    use nalgebra_glm::Vec3;

    fn transform_registry() -> TypeRegistry {
        let mut reg = TypeRegistry {
            types: Default::default(),
        };
        Transform::register(&mut reg);
        reg
    }

    fn transform_struct_info(reg: &TypeRegistry) -> &StructInfo {
        match reg.type_info::<Transform>().unwrap() {
            TypeInfo::Struct(s) => s,
            _ => panic!("expected StructInfo"),
        }
    }

    #[test]
    fn field_lookup() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        assert!(info.field("position").is_some());
        assert!(info.field("missing").is_none());
    }

    #[test]
    fn all_expected_fields_present() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        for name in ["position", "rotation", "scale"] {
            assert!(info.field(name).is_some(), "missing field: {name}");
        }
    }

    #[test]
    fn field_getter_returns_correct_value() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        let field = info.field("position").unwrap();

        let t = Transform::from_xyz(1.0, 2.0, 3.0);
        let pos = field.get::<Vec3>(t.as_reflect()).unwrap();
        assert_eq!(*pos, Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn field_getter_wrong_type_returns_none() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        let field = info.field("position").unwrap();

        let t = Transform::default();
        assert!(field.get::<f32>(t.as_reflect()).is_none());
    }

    #[test]
    fn field_getter_mut_allows_mutation() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        let field = info.field("position").unwrap();

        let mut t = Transform::from_xyz(0.0, 0.0, 0.0);
        let pos = field.get_mut::<Vec3>(t.as_reflect_mut()).unwrap();
        *pos = Vec3::new(9.0, 8.0, 7.0);
        assert_eq!(t.position, Vec3::new(9.0, 8.0, 7.0));
    }

    #[test]
    fn field_setter_updates_value() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        let field = info.field("position").unwrap();

        let mut t = Transform::default();
        field
            .set(t.as_reflect_mut(), Vec3::new(5.0, 6.0, 7.0))
            .unwrap();
        assert_eq!(t.position, Vec3::new(5.0, 6.0, 7.0));
    }

    #[test]
    fn field_setter_wrong_type_returns_none() {
        let reg = transform_registry();
        let info = transform_struct_info(&reg);
        let field = info.field("position").unwrap();

        let mut t = Transform::default();
        assert!(field.set(t.as_reflect_mut(), 42.0f32).is_none());
    }
}
