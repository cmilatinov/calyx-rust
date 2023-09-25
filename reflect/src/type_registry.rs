use crate::trait_meta::TraitMeta;
use crate::type_info::{FieldGetter, FieldSetter, NamedField, StructInfo, TypeInfo};
use crate::{FieldGetterMut, Reflect, ReflectedType, TraitMetaFrom};
use inventory::collect;
use std::any::TypeId;
use std::collections::HashMap;
use utils::{singleton, Init};

pub struct TypeRegistrationFn(pub fn(&mut TypeRegistry));
collect!(TypeRegistrationFn);

pub struct TypeRegistration {
    pub trait_meta: HashMap<TypeId, Box<dyn TraitMeta>>,
    pub type_info: TypeInfo,
}

#[derive(Default)]
pub struct TypeRegistry {
    pub types: HashMap<TypeId, TypeRegistration>,
}
singleton!(TypeRegistry);

impl Init for TypeRegistry {
    fn initialize(&mut self) {
        for f in inventory::iter::<TypeRegistrationFn> {
            (f.0)(self);
        }
    }
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = TypeRegistry::default();
        for f in inventory::iter::<TypeRegistrationFn> {
            (f.0)(&mut registry);
        }
        registry
    }

    pub fn register<T: ReflectedType + 'static>(&mut self) {
        T::register(self)
    }

    pub fn meta<T: 'static>(&mut self) {
        let type_id = TypeId::of::<T>();
        self.types.insert(
            type_id,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::None,
            },
        );
    }

    pub fn meta_struct<T: 'static>(&mut self) -> StructInfoBuilder {
        let type_id = TypeId::of::<T>();
        self.types.insert(
            type_id,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::Struct(StructInfo {
                    type_name: std::any::type_name::<T>(),
                    type_id,
                    fields: HashMap::new(),
                }),
            },
        );
        let registration = self.types.get_mut(&type_id).unwrap();
        if let TypeInfo::Struct(ref mut type_info) = registration.type_info {
            StructInfoBuilder { type_info }
        } else {
            unreachable!()
        }
    }

    pub fn meta_impls<T: Reflect + 'static, M: TraitMeta + TraitMetaFrom<T>>(&mut self) {
        self.types
            .get_mut(&TypeId::of::<T>())
            .and_then(|registration| {
                registration
                    .trait_meta
                    .insert(TypeId::of::<M>(), Box::new(M::trait_meta()))
            });
    }

    pub fn type_info<T: 'static>(&self) -> Option<&TypeInfo> {
        self.type_info_by_id(TypeId::of::<T>())
    }

    pub fn type_info_by_id(&self, id: TypeId) -> Option<&TypeInfo> {
        self.types
            .get(&id)
            .map(|registration| &registration.type_info)
    }

    pub fn type_registration<T: 'static>(&self) -> Option<&TypeRegistration> {
        self.type_registration_by_id(TypeId::of::<T>())
    }

    pub fn type_registration_by_id(&self, id: TypeId) -> Option<&TypeRegistration> {
        self.types.get(&id)
    }

    pub fn trait_meta<T: TraitMeta>(&self, id: TypeId) -> Option<&T> {
        self.type_registration_by_id(id)
            .and_then(|registration| {
                let type_id = TypeId::of::<T>();
                registration.trait_meta.get(&type_id)
            })
            .and_then(|meta| meta.downcast_ref::<T>())
    }

    pub fn list_types<T: TraitMeta>(&self) -> Vec<TypeId> {
        let type_id = TypeId::of::<T>();
        self.types
            .iter()
            .filter(|(_id, reg)| reg.trait_meta.contains_key(&type_id))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn all_of(&self, traits: Vec<TypeId>) -> Vec<TypeId> {
        self.types
            .iter()
            .filter(|(_id, reg)| traits.iter().all(|tid| reg.trait_meta.contains_key(tid)))
            .map(|(id, _)| *id)
            .collect()
    }
}

pub struct StructInfoBuilder<'a> {
    type_info: &'a mut StructInfo,
}

impl<'a> StructInfoBuilder<'a> {
    pub fn field<T: 'static>(
        &mut self,
        name: &'static str,
        getter: FieldGetter,
        getter_mut: FieldGetterMut,
        setter: FieldSetter,
    ) -> &mut StructInfoBuilder<'a> {
        self.type_info.fields.insert(
            name,
            NamedField {
                name,
                type_name: std::any::type_name::<T>(),
                type_id: TypeId::of::<T>(),
                getter,
                getter_mut,
                setter,
            },
        );
        self
    }
}
