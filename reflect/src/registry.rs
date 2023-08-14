use std::any::{Any, TypeId};
use std::collections::HashMap;
use inventory::collect;
use crate::types::{NamedField, StructInfo, TypeInfo};

collect!(TypeRegistrationFn);
pub struct TypeRegistrationFn {
    pub register: fn(&mut TypeRegistry)
}

#[derive(Default)]
pub struct TypeRegistry {
    pub types: HashMap<TypeId, TypeInfo>
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = TypeRegistry::default();

        for f in inventory::iter::<TypeRegistrationFn> {
            (f.register)(&mut registry);
        }

        registry
    }

    pub fn register<T: 'static>(&mut self, info: TypeInfo) {
        self.types.insert(TypeId::of::<T>(), info);
    }
}

pub struct TypeInfoBuilder {
    name: &'static str,
    fields: Vec<(&'static str, &'static str, TypeId)>,
    type_id: TypeId
}

impl TypeInfoBuilder {
    pub fn new<T: 'static>(name: &'static str) -> Self {
        TypeInfoBuilder {
            name,
            fields: Vec::new(),
            type_id: TypeId::of::<T>()
        }
    }

    pub fn add_field(&mut self, name: &'static str, type_name: &'static str, type_id: TypeId) -> &mut Self {
        self.fields.push((name, type_name, type_id));
        self
    }

    pub fn build(&self) -> TypeInfo {
        TypeInfo::Struct(StructInfo {
            name: self.name,
            fields: self.fields.iter().map(|item| NamedField {
                name: item.0,
                type_name: item.1,
                type_id: item.2,
            }).collect(),
            type_id: self.type_id
        })
    }
}
