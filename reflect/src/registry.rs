use std::any::{Any, TypeId};
use std::collections::HashMap;
use inventory::collect;
use crate::types::{FieldGetter, FieldSetter, NamedField, StructInfo, TypeInfo};

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

    pub fn new_struct<T: 'static>(&mut self) -> StructInfoBuilder {
        let type_id = TypeId::of::<T>();
        self.types.insert(type_id, TypeInfo::Struct(StructInfo {
            type_name: "",
            type_id,
            fields: HashMap::new(),
        }));
        let type_info = self.types.get_mut(&type_id).unwrap();
        if let TypeInfo::Struct(ref mut info) = type_info {
            return StructInfoBuilder { info }
        };
        panic!("No type info!");
    }

    pub fn type_info<T: 'static>(&self) -> Option<&TypeInfo> {
        self.types.get(&TypeId::of::<T>())
    }
}

pub struct StructInfoBuilder<'a> {
    info: &'a mut StructInfo,
}

impl<'a> StructInfoBuilder<'a> {
    pub fn new<T: 'static>(info: &'a mut StructInfo) -> StructInfoBuilder<'a> {
        info.type_id = TypeId::of::<T>();
        info.type_name = std::any::type_name::<T>();
        Self {
            info
        }
    }

    pub fn field<T: 'static>(
        &mut self,
        name: &'static str,
        getter: FieldGetter,
        setter: FieldSetter
    ) -> &mut StructInfoBuilder<'a> {
        self.info.fields.insert(name, NamedField {
            name,
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            getter,
            setter
        });
        self
    }
}
