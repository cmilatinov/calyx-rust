use std::any::{TypeId};

#[derive(Clone, Debug)]
pub struct NamedField {
    pub name: &'static str,
    pub type_name: &'static str,
    pub type_id: TypeId,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct UnnamedField {
    index: usize,
    type_name: &'static str,
    type_id: TypeId,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum TypeInfo {
    Struct(StructInfo),
    TupleStruct(TupleStructInfo),
    Tuple(TupleInfo),
}

#[derive(Clone, Debug)]
pub struct TupleStructInfo {
    name: &'static str,
    type_name: &'static str,
    type_id: TypeId,
    fields: Box<[UnnamedField]>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct TupleInfo {
    type_name: &'static str,
    type_id: TypeId,
    fields: Box<[UnnamedField]>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}
#[derive(Clone, Debug)]
pub struct StructInfo {
    pub name: &'static str,
    pub type_id: TypeId,
    pub fields: Vec<NamedField>,
    #[cfg(feature = "documentation")]
    docs: Option<&'static str>,
}

