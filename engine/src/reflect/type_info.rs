use crate::reflect::Reflect;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use uuid::Uuid;

pub type FieldGetter = fn(&dyn Any) -> Option<&dyn Reflect>;
pub type FieldGetterMut = fn(&mut dyn Any) -> Option<&mut dyn Reflect>;
pub type FieldSetter = fn(&mut dyn Any, Box<dyn Any>) -> Option<()>;

#[derive(Copy, Clone)]
pub enum AttributeValue {
    None,
    Float(f64),
    Integer(isize),
    String(&'static str),
}
pub type AttributeMap = HashMap<&'static str, AttributeValue>;

/// Allows you to introspect structs (and potentially other types in the future)
/// at runtime by listing, querying and setting their fields
pub enum TypeInfo {
    Struct(StructInfo),
    None,
}

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
    pub fn get<'a, T: 'static + Reflect>(&'a self, instance: &'a dyn Reflect) -> Option<&'a T> {
        let value = (self.getter)(instance.as_any())?;
        value.downcast_ref::<T>()
    }
    pub fn get_mut<'a, T: 'static + Reflect>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut T> {
        let value = (self.getter_mut)(instance.as_any_mut())?;
        value.downcast_mut::<T>()
    }
    pub fn get_reflect<'a>(&'a self, instance: &'a dyn Reflect) -> Option<&'a dyn Reflect> {
        (self.getter)(instance.as_any())
    }
    pub fn get_reflect_mut<'a>(
        &'a self,
        instance: &'a mut dyn Reflect,
    ) -> Option<&'a mut dyn Reflect> {
        (self.getter_mut)(instance.as_any_mut())
    }
    pub fn set<T: 'static>(&self, instance: &mut dyn Reflect, value: T) -> Option<()> {
        (self.setter)(instance.as_any_mut(), Box::new(value))
    }
    pub fn attr(&self, name: &str) -> Option<AttributeValue> {
        self.attrs.get(name).copied()
    }
}
