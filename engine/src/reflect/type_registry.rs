use crate::reflect::trait_meta::TraitMeta;
use crate::reflect::type_info::{
    EnumInfo, EnumVariantFieldInfo, EnumVariantInfo, FieldGetter, FieldSetter, ListInfo, MapInfo,
    NamedField, OptionInfo, StructInfo, TypeDescriptor, TypeInfo,
};
use crate::reflect::{AttributeMap, FieldGetterMut, Reflect, ReflectedType, TraitMetaFrom};
use crate::utils::TypeUuid;
use inventory::collect;
use std::any::TypeId;
use std::collections::HashMap;
use uuid::Uuid;

/// Inventory entry that registers reflected types into a [`TypeRegistry`].
pub struct TypeRegistrationFn(pub fn(&mut TypeRegistry));
collect!(TypeRegistrationFn);

/// Stored metadata for one reflected type.
#[repr(C)]
pub struct TypeRegistration {
    /// Trait metadata keyed by reflected trait UUID.
    pub trait_meta: HashMap<Uuid, Box<dyn TraitMeta>>,
    /// Structural type information for the reflected type.
    pub type_info: TypeInfo,
}

/// Central registry of reflected types and trait metadata.
#[repr(C)]
pub struct TypeRegistry {
    /// Registered types keyed by stable type UUID.
    pub types: HashMap<Uuid, TypeRegistration>,
}

impl TypeRegistry {
    /// Builds a registry and runs all inventory-based registration functions.
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
    /// Registers a reflected type explicitly.
    pub fn register<T: ReflectedType + 'static>(&mut self) {
        T::register(self)
    }

    /// Creates a placeholder registration entry for `T`.
    pub fn meta<T: TypeUuid + 'static>(&mut self) {
        self.types.insert(
            T::type_uuid(),
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::None,
            },
        );
    }

    /// Begins building struct metadata for `T`.
    pub fn meta_struct<T: TypeUuid + 'static>(
        &mut self,
        attrs: AttributeMap,
    ) -> StructInfoBuilder<'_> {
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

    /// Begins building enum metadata for `T`.
    pub fn meta_enum<T: TypeUuid + 'static>(&mut self, attrs: AttributeMap) -> EnumInfoBuilder<'_> {
        let type_uuid = T::type_uuid();
        self.types.insert(
            type_uuid,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::Enum(EnumInfo {
                    type_name: std::any::type_name::<T>(),
                    type_id: TypeId::of::<T>(),
                    attrs,
                    variants: Vec::new(),
                }),
            },
        );
        let registration = self.types.get_mut(&type_uuid).unwrap();
        if let TypeInfo::Enum(ref mut type_info) = registration.type_info {
            EnumInfoBuilder { type_info }
        } else {
            unreachable!()
        }
    }

    /// Registers list metadata for `T` with element type `E`.
    pub fn meta_list<T: TypeUuid + 'static, E: TypeUuid + 'static>(&mut self) {
        self.types.insert(
            T::type_uuid(),
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::List(ListInfo {
                    type_name: std::any::type_name::<T>(),
                    type_id: TypeId::of::<T>(),
                    element: TypeDescriptor::of::<E>(),
                }),
            },
        );
    }

    /// Registers option metadata for `T` with wrapped value type `V`.
    pub fn meta_option<T: TypeUuid + 'static, V: TypeUuid + 'static>(&mut self) {
        self.types.insert(
            T::type_uuid(),
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::Option(OptionInfo {
                    type_name: std::any::type_name::<T>(),
                    type_id: TypeId::of::<T>(),
                    value: TypeDescriptor::of::<V>(),
                }),
            },
        );
    }

    /// Registers map metadata for `T` with key `K` and value `V`.
    pub fn meta_map<T: TypeUuid + 'static, K: TypeUuid + 'static, V: TypeUuid + 'static>(
        &mut self,
    ) {
        self.types.insert(
            T::type_uuid(),
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info: TypeInfo::Map(MapInfo {
                    type_name: std::any::type_name::<T>(),
                    type_id: TypeId::of::<T>(),
                    key: TypeDescriptor::of::<K>(),
                    value: TypeDescriptor::of::<V>(),
                }),
            },
        );
    }

    /// Attaches reflected trait metadata `M` to `T`.
    pub fn meta_impls<
        T: Reflect + TypeUuid + 'static,
        M: TraitMeta + TraitMetaFrom<T> + TypeUuid + 'static,
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

    /// Looks up type info for `T`.
    pub fn type_info<T: TypeUuid + 'static>(&self) -> Option<&TypeInfo> {
        self.type_info_by_id(T::type_uuid())
    }

    /// Looks up type info by stable type UUID.
    pub fn type_info_by_id(&self, type_uuid: Uuid) -> Option<&TypeInfo> {
        self.types
            .get(&type_uuid)
            .map(|registration| &registration.type_info)
    }

    /// Looks up the full registration entry for `T`.
    pub fn type_registration<T: TypeUuid + 'static>(&self) -> Option<&TypeRegistration> {
        self.type_registration_by_id(T::type_uuid())
    }

    /// Looks up the full registration entry by stable type UUID.
    pub fn type_registration_by_id(&self, type_uuid: Uuid) -> Option<&TypeRegistration> {
        self.types.get(&type_uuid)
    }

    /// Looks up attached trait metadata `T` for the given reflected type UUID.
    pub fn trait_meta<T: TraitMeta + TypeUuid>(&self, type_uuid: Uuid) -> Option<&T> {
        self.type_registration_by_id(type_uuid)
            .and_then(|registration| registration.trait_meta.get(&T::type_uuid()))
            .and_then(|meta| unsafe { Some(&*(meta.as_ref() as *const dyn TraitMeta as *const T)) })
    }

    /// Lists types that implement reflected trait metadata `T`.
    pub fn list_types<T: TraitMeta + TypeUuid>(&self) -> Vec<Uuid> {
        self.types
            .iter()
            .filter(|(_id, reg)| reg.trait_meta.contains_key(&T::type_uuid()))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Lists types that implement all trait UUIDs in `traits`.
    pub fn all_of(&self, traits: Vec<Uuid>) -> Vec<Uuid> {
        self.types
            .iter()
            .filter(|(_id, reg)| traits.iter().all(|tid| reg.trait_meta.contains_key(tid)))
            .map(|(id, _)| *id)
            .collect()
    }
}

impl TypeDescriptor {
    /// Builds a descriptor for type `T`.
    pub fn of<T: TypeUuid + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_uuid: T::type_uuid(),
            type_name: std::any::type_name::<T>(),
        }
    }
}

/// Builder used to populate [`StructInfo`] field metadata.
pub struct StructInfoBuilder<'a> {
    type_info: &'a mut StructInfo,
}

impl<'a> StructInfoBuilder<'a> {
    /// Registers one named field on the target struct metadata.
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

/// Builder used to populate [`EnumInfo`] variant metadata.
pub struct EnumInfoBuilder<'a> {
    type_info: &'a mut EnumInfo,
}

impl<'a> EnumInfoBuilder<'a> {
    /// Registers one enum variant with its field descriptors.
    pub fn variant(
        &mut self,
        name: &'static str,
        fields: Vec<EnumVariantFieldInfo>,
    ) -> &mut EnumInfoBuilder<'a> {
        self.type_info
            .variants
            .push(EnumVariantInfo { name, fields });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as engine;
    use crate::reflect::impls::ReflectDefault;
    use crate::type_uuids;
    use crate::utils::TypeUuid;
    use ReflectedType;

    // Minimal stub type with a known UUID
    #[derive(Default, TypeUuid, Reflect)]
    #[reflect(Default)]
    #[allow(unused)]
    #[repr(C)]
    struct Foo {
        data: f32,
    }

    #[derive(TypeUuid)]
    #[uuid = "57f8272e-6073-4308-acf3-b1f51d3a35bf"]
    struct ListWrapper;

    #[derive(TypeUuid)]
    #[uuid = "277cb37f-71c9-4323-9a87-f3accb887f5f"]
    struct OptionWrapper;

    #[derive(TypeUuid)]
    #[uuid = "6dd3cd58-2854-4421-82e6-f1e0b7ea7efd"]
    struct MapWrapper;

    fn empty_registry() -> TypeRegistry {
        TypeRegistry {
            types: Default::default(),
        }
    }

    fn foo_registry() -> TypeRegistry {
        let mut registry = TypeRegistry {
            types: Default::default(),
        };
        Foo::register(&mut registry);
        registry
    }

    #[test]
    fn meta_registers_type() {
        let reg = foo_registry();
        assert!(reg.type_registration::<Foo>().is_some());
    }

    #[test]
    fn missing_type_returns_none() {
        let reg = empty_registry();
        assert!(reg.type_registration::<Foo>().is_none());
    }

    #[test]
    fn meta_struct_stores_type_info() {
        let reg = foo_registry();
        let info = reg.type_info::<Foo>().unwrap();
        assert!(matches!(info, TypeInfo::Struct(_)));
    }

    #[test]
    fn type_info_by_id_works() {
        let reg = foo_registry();
        let info = reg.type_info_by_id(Foo::type_uuid());
        assert!(info.is_some());
    }

    #[test]
    fn list_types_finds_registered_trait() {
        let reg = empty_registry();
        assert!(reg.list_types::<ReflectDefault>().is_empty());
        let reg = foo_registry();
        assert_eq!(reg.list_types::<ReflectDefault>().len(), 1);
    }

    #[test]
    fn all_of_traits_expected_behavior() {
        let reg = empty_registry();
        let result = reg.all_of(vec![]);
        assert!(result.is_empty());

        let reg = foo_registry();
        let result = reg.all_of(vec![]);
        assert!(result.contains(&Foo::type_uuid()));

        let result = reg.all_of(type_uuids!(ReflectDefault));
        assert!(result.contains(&Foo::type_uuid()));
    }

    #[test]
    fn meta_list_stores_element_type() {
        let mut reg = empty_registry();
        reg.meta_list::<ListWrapper, f32>();

        let TypeInfo::List(info) = reg.type_info::<ListWrapper>().unwrap() else {
            panic!("expected list info");
        };
        assert_eq!(info.element.type_uuid, f32::type_uuid());
    }

    #[test]
    fn meta_option_stores_value_type() {
        let mut reg = empty_registry();
        reg.meta_option::<OptionWrapper, u32>();

        let TypeInfo::Option(info) = reg.type_info::<OptionWrapper>().unwrap() else {
            panic!("expected option info");
        };
        assert_eq!(info.value.type_uuid, u32::type_uuid());
    }

    #[test]
    fn meta_map_stores_key_and_value_types() {
        let mut reg = empty_registry();
        reg.meta_map::<MapWrapper, String, f32>();

        let TypeInfo::Map(info) = reg.type_info::<MapWrapper>().unwrap() else {
            panic!("expected map info");
        };
        assert_eq!(info.key.type_uuid, String::type_uuid());
        assert_eq!(info.value.type_uuid, f32::type_uuid());
    }
}
