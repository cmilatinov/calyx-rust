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

/// Inventory entry that registers one reflected type into a [`TypeRegistry`].
///
/// `#[derive(Reflect)]` and `impl_reflect_value!` submit one per type, and
/// [`TypeRegistry::new`] runs them all.
pub struct ReflectRegistrationFn {
    /// Crate that produced this registration.
    pub crate_name: &'static str,
    /// Module path that produced this registration.
    pub module_path: &'static str,
    /// Debug-friendly name for the registration source.
    pub name: &'static str,
    /// Callback that inserts one or more types into a [`TypeRegistry`].
    pub function: fn(&mut TypeRegistry),
}
collect!(ReflectRegistrationFn);

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
    /// Rust type identities behind registered UUIDs, used to detect collisions.
    ///
    /// Kept outside [`TypeRegistration`] and appended after `types` so the layout
    /// of `types` stays stable for already-built plugin assemblies.
    pub identities: RegistryIdentities,
}

/// Rust type name recorded for a registered UUID, with the registration pass
/// that recorded it.
///
/// Compared by name rather than `TypeId` because a rebuilt plugin assembly can
/// assign a new `TypeId` to the same source type. Owned so it survives unloading
/// the assembly that registered it.
pub type TypeIdentity = (String, u64);

/// Registry-wide record of which Rust type owns each registered UUID.
#[repr(C)]
#[derive(Default)]
pub struct RegistryIdentities {
    /// Reflected types keyed by type UUID.
    pub types: HashMap<Uuid, TypeIdentity>,
    /// Trait metadata types keyed by trait metadata UUID.
    pub trait_meta: HashMap<Uuid, TypeIdentity>,
    /// Current registration pass. Pass 0 is the host; each plugin load starts a new one.
    pub generation: u64,
}

impl RegistryIdentities {
    /// Generation to record for a UUID owner, given any existing owner.
    fn owner_generation(&self, existing: Option<&TypeIdentity>) -> u64 {
        // Host-owned UUIDs stay protected when a plugin re-registers the same type.
        match existing {
            Some((_, 0)) => 0,
            _ => self.generation,
        }
    }

    /// Whether registering `name` conflicts with the recorded owner of its UUID.
    ///
    /// A different type conflicts unless its previous owner came from an earlier
    /// plugin pass, since a rebuilt plugin may rename or move its types.
    fn conflicts(&self, existing: &TypeIdentity, name: &str) -> bool {
        existing.0 != name && (existing.1 == 0 || existing.1 >= self.generation)
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeRegistry {
    /// Builds a registry holding every reflected type linked into this binary.
    pub fn new() -> Self {
        let mut registry = Self {
            types: Default::default(),
            identities: Default::default(),
        };
        let count = registry.run_registrations(|_| true);
        log::info!("Applied {count} reflection registration callbacks");
        registry
    }

    /// Registers every reflected type declared by the crate named `crate_name`.
    ///
    /// Plugin entry points call this with `env!("CARGO_PKG_NAME")` so they add
    /// only their own types to the host registry.
    pub fn register_crate(&mut self, crate_name: &str) {
        self.run_registrations(|f| f.crate_name == crate_name);
    }

    /// Runs the inventory registrations accepted by `filter`, returning how many ran.
    fn run_registrations(&mut self, filter: impl Fn(&ReflectRegistrationFn) -> bool) -> usize {
        let mut count = 0;
        for f in inventory::iter::<ReflectRegistrationFn>().filter(|f| filter(f)) {
            log::trace!(
                "Registering reflected types from {}::{} ({})",
                f.crate_name,
                f.module_path,
                f.name
            );
            (f.function)(self);
            count += 1;
        }
        count
    }
}

impl TypeRegistry {
    /// Starts a new plugin registration pass. Types registered by earlier plugin
    /// passes may then be replaced by differently named types with the same UUID.
    pub fn begin_plugin_load(&mut self) {
        self.identities.generation += 1;
    }

    /// Registers a reflected type explicitly.
    pub fn register<T: ReflectedType + 'static>(&mut self) {
        T::register(self)
    }

    /// Creates a placeholder registration entry for `T`.
    pub fn meta<T: TypeUuid + 'static>(&mut self) {
        self.insert_registration::<T>(TypeInfo::None);
    }

    /// Begins building struct metadata for `T`.
    pub fn meta_struct<T: TypeUuid + 'static>(
        &mut self,
        attrs: AttributeMap,
    ) -> StructInfoBuilder<'_> {
        let type_uuid = T::type_uuid();
        self.insert_registration::<T>(TypeInfo::Struct(StructInfo {
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            attrs,
            fields: HashMap::new(),
        }));
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
        self.insert_registration::<T>(TypeInfo::Enum(EnumInfo {
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            attrs,
            variants: Vec::new(),
        }));
        let registration = self.types.get_mut(&type_uuid).unwrap();
        if let TypeInfo::Enum(ref mut type_info) = registration.type_info {
            EnumInfoBuilder { type_info }
        } else {
            unreachable!()
        }
    }

    /// Registers list metadata for `T` with element type `E`.
    pub fn meta_list<T: TypeUuid + 'static, E: TypeUuid + 'static>(&mut self) {
        self.insert_registration::<T>(TypeInfo::List(ListInfo {
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            element: TypeDescriptor::of::<E>(),
        }));
    }

    /// Registers option metadata for `T` with wrapped value type `V`.
    pub fn meta_option<T: TypeUuid + 'static, V: TypeUuid + 'static>(&mut self) {
        self.insert_registration::<T>(TypeInfo::Option(OptionInfo {
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            value: TypeDescriptor::of::<V>(),
        }));
    }

    /// Registers map metadata for `T` with key `K` and value `V`.
    pub fn meta_map<T: TypeUuid + 'static, K: TypeUuid + 'static, V: TypeUuid + 'static>(
        &mut self,
    ) {
        self.insert_registration::<T>(TypeInfo::Map(MapInfo {
            type_name: std::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            key: TypeDescriptor::of::<K>(),
            value: TypeDescriptor::of::<V>(),
        }));
    }

    /// Attaches reflected trait metadata `M` to `T`.
    pub fn meta_impls<
        T: Reflect + TypeUuid + 'static,
        M: TraitMeta + TraitMetaFrom<T> + TypeUuid + 'static,
    >(
        &mut self,
    ) {
        if !self.types.contains_key(&T::type_uuid()) {
            return;
        }
        let meta_uuid = M::type_uuid();
        let meta_identity = std::any::type_name::<M>();
        if let Some(existing) = self.identities.trait_meta.get(&meta_uuid) {
            if self.identities.conflicts(existing, meta_identity) {
                panic!(
                    "duplicate trait metadata UUID {meta_uuid}: `{}` conflicts with already registered `{}`",
                    meta_identity, existing.0
                );
            }
        }
        let replaced = self
            .identities
            .trait_meta
            .get(&meta_uuid)
            .is_some_and(|existing| existing.0 != meta_identity);
        if replaced {
            // Stale registrations from earlier plugin passes may still hold the
            // old metadata object, which must not be read back as the new type.
            for registration in self.types.values_mut() {
                registration.trait_meta.remove(&meta_uuid);
            }
        }
        let generation = self
            .identities
            .owner_generation(self.identities.trait_meta.get(&meta_uuid));
        self.identities
            .trait_meta
            .insert(meta_uuid, (meta_identity.to_string(), generation));
        if let Some(registration) = self.types.get_mut(&T::type_uuid()) {
            registration
                .trait_meta
                .insert(meta_uuid, Box::new(M::trait_meta()));
        }
    }

    /// Inserts a fresh registration for `T`, panicking if its UUID already
    /// belongs to a different type. Re-registering the same type replaces it.
    fn insert_registration<T: TypeUuid + 'static>(&mut self, type_info: TypeInfo) {
        let type_uuid = T::type_uuid();
        let type_name = std::any::type_name::<T>();
        if let Some(existing) = self.identities.types.get(&type_uuid) {
            if self.identities.conflicts(existing, type_name) {
                panic!(
                    "duplicate type UUID {type_uuid}: `{type_name}` conflicts with already registered `{}`",
                    existing.0
                );
            }
        }
        let generation = self
            .identities
            .owner_generation(self.identities.types.get(&type_uuid));
        self.identities
            .types
            .insert(type_uuid, (type_name.to_string(), generation));
        self.types.insert(
            type_uuid,
            TypeRegistration {
                trait_meta: HashMap::new(),
                type_info,
            },
        );
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
            .map(|meta| unsafe { &*(meta.as_ref() as *const dyn TraitMeta as *const T) })
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
            identities: Default::default(),
        }
    }

    fn foo_registry() -> TypeRegistry {
        let mut registry = TypeRegistry {
            types: Default::default(),
            identities: Default::default(),
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

    #[derive(TypeUuid)]
    #[uuid = "3b0c2f5e-8d4a-4f6e-9a1b-7c5d2e8f1a40"]
    struct DupA;

    #[derive(TypeUuid)]
    #[uuid = "3b0c2f5e-8d4a-4f6e-9a1b-7c5d2e8f1a40"]
    struct DupB;

    #[derive(Default, TypeUuid, Reflect)]
    #[allow(unused)]
    #[repr(C)]
    struct Bar {
        data: f32,
    }

    #[derive(TypeUuid)]
    #[uuid = "9e1d7a62-5b3c-4c8f-a2d4-0f6b8c3e7a15"]
    struct DupMetaA;

    #[derive(TypeUuid)]
    #[uuid = "9e1d7a62-5b3c-4c8f-a2d4-0f6b8c3e7a15"]
    struct DupMetaB;

    impl TraitMeta for DupMetaA {}
    impl TraitMeta for DupMetaB {}
    impl<T: Reflect> TraitMetaFrom<T> for DupMetaA {
        fn trait_meta() -> Self {
            DupMetaA
        }
    }
    impl<T: Reflect> TraitMetaFrom<T> for DupMetaB {
        fn trait_meta() -> Self {
            DupMetaB
        }
    }

    fn panic_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let err = std::panic::catch_unwind(f).expect_err("expected a panic");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    #[test]
    fn duplicate_uuid_for_different_type_is_rejected_with_both_names() {
        let mut reg = empty_registry();
        reg.meta::<DupA>();
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta_struct::<DupB>(AttributeMap::new());
        }));
        assert!(msg.contains("DupA") && msg.contains("DupB"), "{msg}");

        let mut reg = empty_registry();
        reg.meta_struct::<DupA>(AttributeMap::new());
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta_enum::<DupB>(AttributeMap::new());
        }));
        assert!(msg.contains("DupA") && msg.contains("DupB"), "{msg}");
    }

    #[test]
    fn reregistering_same_type_is_idempotent() {
        let mut reg = foo_registry();
        Foo::register(&mut reg);
        assert_eq!(reg.types.len(), 1);
        assert!(reg.type_registration::<Foo>().is_some());
        assert_eq!(reg.list_types::<ReflectDefault>().len(), 1);
    }

    #[test]
    fn duplicate_trait_meta_uuid_is_rejected_with_both_names() {
        let mut reg = empty_registry();
        reg.meta::<DupA>();
        reg.meta_impls::<Foo, DupMetaA>();
        Foo::register(&mut reg);
        reg.meta_impls::<Foo, DupMetaA>();
        reg.meta_impls::<Foo, DupMetaA>();
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta_impls::<Foo, DupMetaB>();
        }));
        assert!(
            msg.contains("DupMetaA") && msg.contains("DupMetaB"),
            "{msg}"
        );
    }

    #[test]
    fn duplicate_trait_meta_uuid_is_rejected_across_target_types() {
        let mut reg = foo_registry();
        Bar::register(&mut reg);
        reg.meta_impls::<Foo, DupMetaA>();
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta_impls::<Bar, DupMetaB>();
        }));
        assert!(
            msg.contains("DupMetaA") && msg.contains("DupMetaB"),
            "{msg}"
        );
    }

    #[test]
    fn plugin_reload_may_replace_a_renamed_plugin_type_but_not_a_host_type() {
        let mut reg = empty_registry();
        reg.meta::<DupA>();
        reg.begin_plugin_load();
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta::<DupB>();
        }));
        assert!(msg.contains("DupA") && msg.contains("DupB"), "{msg}");

        let mut reg = empty_registry();
        reg.begin_plugin_load();
        reg.meta::<DupA>();
        reg.begin_plugin_load();
        reg.meta::<DupB>();
        assert!(reg.type_registration::<DupB>().is_some());
    }

    #[test]
    fn plugin_reusing_a_host_trait_meta_keeps_it_protected() {
        let mut reg = foo_registry();
        reg.meta_impls::<Foo, DupMetaA>();
        reg.begin_plugin_load();
        reg.meta_impls::<Foo, DupMetaA>();
        reg.begin_plugin_load();
        let msg = panic_message(std::panic::AssertUnwindSafe(|| {
            reg.meta_impls::<Foo, DupMetaB>();
        }));
        assert!(
            msg.contains("DupMetaA") && msg.contains("DupMetaB"),
            "{msg}"
        );
    }

    #[test]
    fn replacing_plugin_trait_meta_purges_stale_metadata_objects() {
        let mut reg = foo_registry();
        reg.begin_plugin_load();
        reg.meta_impls::<Foo, DupMetaA>();
        reg.begin_plugin_load();
        Bar::register(&mut reg);
        reg.meta_impls::<Bar, DupMetaB>();
        assert!(reg.trait_meta::<DupMetaB>(Foo::type_uuid()).is_none());
        assert!(reg.trait_meta::<DupMetaB>(Bar::type_uuid()).is_some());
    }

    #[test]
    fn new_registers_derived_and_value_types() {
        let reg = TypeRegistry::new();
        assert!(reg.type_registration::<Foo>().is_some());
        assert!(reg.type_registration::<f32>().is_some());
    }
}
