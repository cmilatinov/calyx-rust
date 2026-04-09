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
