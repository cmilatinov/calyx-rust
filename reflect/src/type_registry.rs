use std::collections::HashMap;

use inventory::collect;
use uuid::Uuid;

use crate::trait_meta::TraitMeta;
use crate::type_info::{FieldGetter, FieldSetter, NamedField, StructInfo, TypeInfo};
use crate::{AttributeMap, FieldGetterMut, Reflect, ReflectedType, TraitMetaFrom, TypeUuid};

pub struct TypeRegistrationFn(pub fn(&mut TypeRegistry));
collect!(TypeRegistrationFn);

pub struct TypeRegistration {
    pub trait_meta: HashMap<Uuid, Box<dyn TraitMeta>>,
    pub type_info: TypeInfo,
}

#[derive(Default)]
pub struct TypeRegistry {
    pub types: HashMap<Uuid, TypeRegistration>,
}

impl TypeRegistry {
    pub fn register<T: ReflectedType + 'static>(&mut self) {
        T::register(self)
    }

    pub fn meta<T: TypeUuid + 'static>(&mut self) {
        let uuid = T::type_uuid();
        self.types.insert(
            uuid,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::None,
            },
        );
    }

    pub fn meta_struct<T: TypeUuid + 'static>(&mut self, attrs: AttributeMap) -> StructInfoBuilder {
        let type_uuid = T::type_uuid();
        self.types.insert(
            type_uuid,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::Struct(StructInfo {
                    type_name: std::any::type_name::<T>(),
                    type_uuid,
                    attrs,
                    fields: HashMap::new(),
                }),
            },
        );
        let registration = self.types.get_mut(&type_uuid).unwrap();
        if let TypeInfo::Struct(ref mut type_info) = registration.type_info {
            StructInfoBuilder { type_info }
        } else {
            unreachable!()
        }
    }

    pub fn meta_impls<
        T: TypeUuid + Reflect + 'static,
        M: TypeUuid + TraitMeta + TraitMetaFrom<T>,
    >(
        &mut self,
    ) {
        let uuid = T::type_uuid();
        self.types.get_mut(&uuid).and_then(|registration| {
            registration
                .trait_meta
                .insert(M::type_uuid(), Box::new(M::trait_meta()))
        });
    }

    pub fn type_info<T: TypeUuid + 'static>(&self) -> Option<&TypeInfo> {
        self.type_info_by_uuid(T::type_uuid())
    }

    pub fn type_info_by_uuid(&self, uuid: Uuid) -> Option<&TypeInfo> {
        self.types
            .get(&uuid)
            .map(|registration| &registration.type_info)
    }

    pub fn type_registration<T: TypeUuid + 'static>(&self) -> Option<&TypeRegistration> {
        self.type_registration_by_uuid(T::type_uuid())
    }

    pub fn type_registration_by_uuid(&self, uuid: Uuid) -> Option<&TypeRegistration> {
        self.types.get(&uuid)
    }

    pub fn trait_meta<T: TraitMeta + TypeUuid>(&self, uuid: Uuid) -> Option<&T> {
        self.type_registration_by_uuid(uuid)
            .and_then(|registration| {
                let uuid = T::type_uuid();
                registration.trait_meta.get(&uuid)
            })
            .and_then(|meta| meta.downcast_ref::<T>())
    }

    pub fn list_types<T: TraitMeta + TypeUuid>(&self) -> Vec<Uuid> {
        let uuid = T::type_uuid();
        self.types
            .iter()
            .filter(|(_id, reg)| reg.trait_meta.contains_key(&uuid))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn all_of(&self, traits: Vec<Uuid>) -> Vec<Uuid> {
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
    pub fn field<T: TypeUuid + 'static>(
        &mut self,
        name: &'static str,
        attrs: AttributeMap,
        doc: Option<&'static str>,
        getter: FieldGetter,
        getter_mut: FieldGetterMut,
        setter: FieldSetter,
    ) -> &mut StructInfoBuilder<'a> {
        self.type_info.fields.insert(
            name,
            NamedField {
                name,
                type_name: std::any::type_name::<T>(),
                type_uuid: T::type_uuid(),
                attrs,
                doc,
                getter,
                getter_mut,
                setter,
            },
        );
        self
    }
}
