use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::error::Error;

pub type FieldGetter = Box<dyn Fn(&dyn Any) -> Option<&dyn Any>>;
pub type FieldSetter = Box<dyn Fn(&mut dyn Any, Box<dyn Any>) -> Option<()>>;

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
    pub fn get<'a, T: 'static>(&'a self, instance: &'a dyn Any) -> Option<&'a T> {
        let value = (self.getter)(instance)?;
        value.downcast_ref::<T>()
    }
    pub fn set<T: 'static>(&self, instance: &mut dyn Any, value: T) -> Option<()> {
        (self.setter)(instance, Box::new(value))
    }
}

pub struct UnnamedField {
    index: usize,
    type_id: TypeId,
    type_name: &'static str,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

pub enum TypeInfo {
    Struct(StructInfo),
    TupleStruct(TupleStructInfo),
    Tuple(TupleInfo),
    None
}

pub struct TupleStructInfo {
    name: &'static str,
    type_id: TypeId,
    type_name: &'static str,
    fields: Box<[UnnamedField]>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

pub struct TupleInfo {
    type_id: TypeId,
    type_name: &'static str,
    fields: Box<[UnnamedField]>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
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
