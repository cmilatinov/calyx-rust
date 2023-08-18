use std::any::{Any, TypeId};
use std::collections::HashMap;
use crate::reflect::Reflect;

pub type FieldGetter = fn(&dyn Any) -> Option<&dyn Any>;
pub type FieldSetter = fn(&mut dyn Any, Box<dyn Any>) -> Option<()>;

/// Allows you to introspect structs (and potentially other types in the future)
/// at runtime by listing, querying and setting their fields
pub enum TypeInfo {
    Struct(StructInfo),
    None
}

pub struct StructInfo {
    pub type_name: &'static str,
    pub type_id: TypeId,
    pub fields: HashMap<&'static str, NamedField>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

impl StructInfo {
    pub fn field(&self, name: &str) -> Option<&NamedField> {
        self.fields.get(name)
    }
}

pub struct NamedField {
    pub name: &'static str,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub getter: FieldGetter,
    pub setter: FieldSetter,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

impl NamedField {
    pub fn get<'a, T: 'static>(&'a self, instance: &'a dyn Reflect) -> Option<&'a T> {
        let value = (self.getter)(instance.as_any())?;
        value.downcast_ref::<T>()
    }
    pub fn set<T: 'static>(&self, instance: &mut dyn Reflect, value: T) -> Option<()> {
        (self.setter)(instance.as_any_mut(), Box::new(value))
    }
}
