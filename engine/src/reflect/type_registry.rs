use crate::reflect::trait_meta::TraitMeta;
use crate::reflect::type_info::{FieldGetter, FieldSetter, NamedField, StructInfo, TypeInfo};
use crate::reflect::{AttributeMap, FieldGetterMut, Reflect, ReflectedType, TraitMetaFrom};
use crate::utils::TypeUuid;
use inventory::collect;
use std::any::TypeId;
use std::collections::HashMap;
use uuid::Uuid;

pub struct TypeRegistrationFn(pub fn(&mut TypeRegistry));
collect!(TypeRegistrationFn);

pub struct TypeRegistration {
    pub trait_meta: HashMap<Uuid, Box<dyn TraitMeta>>,
    pub type_info: TypeInfo,
}

pub struct TypeRegistry {
    pub types: HashMap<Uuid, TypeRegistration>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            types: Default::default(),
        };
        for f in inventory::iter::<TypeRegistrationFn> {
            f.0(&mut registry)
        }
        registry
    }
}

impl TypeRegistry {
    pub fn register<T: ReflectedType + 'static>(&mut self) {
        T::register(self)
    }

    pub fn meta<T: TypeUuid + 'static>(&mut self) {
        self.types.insert(
            T::type_uuid(),
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
                    type_id: TypeId::of::<T>(),
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
        T: Reflect + TypeUuid + 'static,
        M: TraitMeta + TraitMetaFrom<T> + TypeUuid,
    >(
        &mut self,
    ) {
        self.types
            .get_mut(&T::type_uuid())
            .and_then(|registration| {
                registration
                    .trait_meta
                    .insert(M::type_uuid(), Box::new(M::trait_meta()))
            });
    }

    pub fn type_info<T: TypeUuid + 'static>(&self) -> Option<&TypeInfo> {
        self.type_info_by_id(T::type_uuid())
    }

    pub fn type_info_by_id(&self, type_uuid: Uuid) -> Option<&TypeInfo> {
        self.types
            .get(&type_uuid)
            .map(|registration| &registration.type_info)
    }

    pub fn type_registration<T: TypeUuid + 'static>(&self) -> Option<&TypeRegistration> {
        self.type_registration_by_id(T::type_uuid())
    }

    pub fn type_registration_by_id(&self, type_uuid: Uuid) -> Option<&TypeRegistration> {
        self.types.get(&type_uuid)
    }

    pub fn trait_meta<T: TraitMeta + TypeUuid>(&self, type_uuid: Uuid) -> Option<&T> {
        self.type_registration_by_id(type_uuid)
            .and_then(|registration| registration.trait_meta.get(&T::type_uuid()))
            .and_then(|meta| unsafe { Some(&*(meta.as_ref() as *const dyn TraitMeta as *const T)) })
    }

    pub fn list_types<T: TraitMeta + TypeUuid>(&self) -> Vec<Uuid> {
        self.types
            .iter()
            .filter(|(_id, reg)| reg.trait_meta.contains_key(&T::type_uuid()))
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
                type_id: TypeId::of::<T>(),
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
