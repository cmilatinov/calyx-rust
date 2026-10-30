use crate::reflect::trait_meta::TraitMeta;
use crate::reflect::type_info::{
    EnumInfo, EnumVariantFieldInfo, EnumVariantInfo, FieldGetter, FieldSetter, ListInfo, MapInfo,
    NamedField, OptionInfo, StructInfo, TypeDescriptor, TypeInfo,
};
use crate::reflect::{
    AttributeMap, FieldGetterMut, Reflect, ReflectDefault, ReflectedType, TraitMetaFrom,
};
use crate::utils::TypeUuid;
use indexmap::IndexMap;
use inventory::collect;
use std::any::{Any, TypeId};
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
pub struct TypeRegistration {
    /// Trait metadata keyed by reflected trait UUID.
    pub trait_meta: HashMap<Uuid, Box<dyn TraitMeta>>,
    /// Structural type information for the reflected type.
    pub type_info: TypeInfo,
}

/// Central registry of reflected types and trait metadata.
pub struct TypeRegistry {
    /// Registered types keyed by stable type UUID.
    pub types: HashMap<Uuid, TypeRegistration>,
    /// Rust type identities behind registered UUIDs, used to detect collisions.
    pub identities: RegistryIdentities,
}

/// Rust type name recorded for a registered UUID, with the registration pass
/// that recorded it.
///
/// Compared by name rather than `TypeId` because a rebuilt plugin assembly can
/// assign a new `TypeId` to the same source type. Owned so it survives unloading
/// the assembly that registered it.
pub type TypeIdentity = (String, u64);

/// The engine build a plugin assembly links, as seen from the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEngineBuild {
    /// The host's engine build: engine types share their `TypeId`s.
    Same,
    /// Another engine build: every engine type has another `TypeId` there, so
    /// the engine components the plugin reads, writes or adds are not the
    /// host's.
    Other,
    /// The plugin never ran [`TypeRegistry::register_crate`], either because
    /// it predates this check or registers its types another way.
    Unknown,
}

/// Registry-wide record of which Rust type owns each registered UUID.
#[derive(Default)]
pub struct RegistryIdentities {
    /// Reflected types keyed by type UUID.
    pub types: HashMap<Uuid, TypeIdentity>,
    /// Trait metadata types keyed by trait metadata UUID.
    pub trait_meta: HashMap<Uuid, TypeIdentity>,
    /// Current registration pass. Pass 0 is the host; each plugin load starts a new one.
    pub generation: u64,
    /// `TypeId` of [`TypeRegistry`] in the engine build that ran the latest
    /// [`TypeRegistry::register_crate`], so the host can tell whether a plugin
    /// assembly links the same engine build.
    pub registering_engine: Option<TypeId>,
    /// The host's logger, recorded by [`TypeRegistry::begin_plugin_load`] so
    /// that [`TypeRegistry::register_crate`] can install it in the plugin
    /// assembly's own copy of the `log` crate, whose records would otherwise
    /// go nowhere.
    pub host_logger: Option<HostLogger>,
}

/// A logger and the most verbose level it accepts, handed from the host to a
/// plugin assembly.
#[derive(Clone, Copy)]
pub struct HostLogger {
    /// The host's installed logger.
    pub logger: &'static dyn log::Log,
    /// The host's maximum log level.
    pub max_level: log::LevelFilter,
}

impl HostLogger {
    /// The logger and level installed in this binary's copy of `log`.
    fn current() -> Self {
        Self {
            logger: log::logger(),
            max_level: log::max_level(),
        }
    }

    /// Installs this logger in this binary's copy of `log`, unless it is the
    /// one already installed there, as it is in the host, or another logger
    /// already is.
    fn install(self) {
        let current = log::logger() as *const dyn log::Log;
        if std::ptr::addr_eq(self.logger as *const dyn log::Log, current) {
            return;
        }
        if log::set_logger(self.logger).is_ok() {
            log::set_max_level(self.max_level);
        }
    }
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
        self.identities.registering_engine = Some(TypeId::of::<TypeRegistry>());
        if let Some(logger) = self.identities.host_logger {
            logger.install();
        }
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
        self.identities.registering_engine = None;
        self.identities.host_logger = Some(HostLogger::current());
    }

    /// Drops every registration plugin passes made, including the host types
    /// they registered again, leaving the types linked into this binary as
    /// [`Self::new`] registers them.
    ///
    /// A plugin's registrations hold type names, trait objects and function
    /// pointers in its assembly, so call this while that assembly is still
    /// loaded, before unloading it.
    pub fn unload_plugins(&mut self) {
        let generation = self.identities.generation;
        *self = Self::new();
        self.identities.generation = generation;
    }

    /// Which engine build the plugin loaded since [`Self::begin_plugin_load`]
    /// registered its types through.
    pub fn plugin_engine_build(&self) -> PluginEngineBuild {
        match self.identities.registering_engine {
            None => PluginEngineBuild::Unknown,
            Some(engine) if engine == TypeId::of::<TypeRegistry>() => PluginEngineBuild::Same,
            Some(_) => PluginEngineBuild::Other,
        }
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
            fields: IndexMap::new(),
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
    ///
    /// Metadata stored under `T`'s UUID that is not a `T` is not returned.
    pub fn trait_meta<T: TraitMeta + TypeUuid>(&self, type_uuid: Uuid) -> Option<&T> {
        self.type_registration_by_id(type_uuid)
            .and_then(|registration| registration.trait_meta.get(&T::type_uuid()))
            .and_then(|meta| {
                let meta: &dyn Any = meta.as_ref();
                meta.downcast_ref()
            })
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

impl TypeRegistry {
    /// Registers list metadata and [`ReflectDefault`] for the list type `Vec<E>`
    /// that a reflected field holds.
    ///
    /// Many structs share a list type such as `Vec<f32>`, so the first
    /// registration in the host pass owns it. A plugin pass leaves a host-owned
    /// list alone: replacing it would leave its type name pointing into the
    /// plugin's assembly after that assembly unloads.
    pub fn meta_field_list<E: Reflect + TypeUuid + 'static>(&mut self) {
        let host_owned = self
            .identities
            .types
            .get(&Vec::<E>::type_uuid())
            .is_some_and(|(_, generation)| *generation == 0);
        if host_owned && self.identities.generation > 0 {
            return;
        }
        self.meta_list::<Vec<E>, E>();
        self.meta_impls::<Vec<E>, ReflectDefault>();
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
    use crate as calyx_engine;
    use crate::reflect::impls::{ReflectDefault, ReflectGenericFloat};
    use crate::type_uuids;
    use crate::utils::TypeUuid;
    use ReflectedType;

    // Minimal stub type with a known UUID
    #[derive(Default, TypeUuid, Reflect)]
    #[reflect(Default)]
    #[allow(unused)]
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

    // Two trait metadata types named `ProbeMeta`, which the `TypeUuid` derive
    // gives the same type UUID since neither has a `#[uuid]` attribute.
    mod first {
        use crate as calyx_engine;
        use crate::reflect::TraitMeta;
        use crate::utils::TypeUuid;

        #[derive(TypeUuid)]
        pub struct ProbeMeta(pub u32);

        impl TraitMeta for ProbeMeta {}
    }

    mod second {
        use crate as calyx_engine;
        use crate::reflect::TraitMeta;
        use crate::utils::TypeUuid;

        #[derive(TypeUuid)]
        pub struct ProbeMeta;

        impl TraitMeta for ProbeMeta {}
    }

    #[test]
    fn trait_meta_tells_same_named_metadata_apart() {
        let shared = first::ProbeMeta::type_uuid();
        assert_eq!(shared, second::ProbeMeta::type_uuid());
        let mut registry = foo_registry();
        registry
            .types
            .get_mut(&Foo::type_uuid())
            .unwrap()
            .trait_meta
            .insert(shared, Box::new(first::ProbeMeta(9)));

        assert!(registry
            .trait_meta::<second::ProbeMeta>(Foo::type_uuid())
            .is_none());
        assert_eq!(
            registry
                .trait_meta::<first::ProbeMeta>(Foo::type_uuid())
                .map(|meta| meta.0),
            Some(9)
        );
    }

    #[test]
    fn plugin_registering_through_this_engine_links_it() {
        let mut registry = empty_registry();
        registry.begin_plugin_load();
        registry.register_crate("no_such_crate");
        assert_eq!(registry.plugin_engine_build(), PluginEngineBuild::Same);
    }

    #[test]
    fn unloading_plugins_leaves_only_the_host_registrations() {
        let mut reg = TypeRegistry::new();
        let host_types = reg.types.len();
        reg.begin_plugin_load();
        // A type the next build may remove, and a host type the plugin
        // registered again with its own metadata.
        reg.meta::<DupA>();
        reg.meta::<Foo>();

        reg.unload_plugins();

        assert!(reg.type_registration::<DupA>().is_none());
        assert!(!reg.identities.types.contains_key(&DupA::type_uuid()));
        assert!(matches!(reg.type_info::<Foo>(), Some(TypeInfo::Struct(_))));
        assert_eq!(reg.types.len(), host_types);
        reg.begin_plugin_load();
        assert_eq!(reg.identities.generation, 2);
    }

    #[test]
    fn plugin_registering_through_another_engine_is_detected() {
        let mut registry = empty_registry();
        registry.begin_plugin_load();
        // Another engine build's `TypeId` for `TypeRegistry` is just a
        // different `TypeId`.
        registry.identities.registering_engine = Some(TypeId::of::<ListWrapper>());
        assert_eq!(registry.plugin_engine_build(), PluginEngineBuild::Other);
    }

    #[test]
    fn plugin_that_never_reports_its_engine_is_unknown() {
        let mut registry = empty_registry();
        registry.begin_plugin_load();
        registry.register_crate("no_such_crate");
        // An assembly built before the marker existed, or one that skips
        // `register_crate`, leaves it unset for the next load.
        registry.begin_plugin_load();
        assert_eq!(registry.plugin_engine_build(), PluginEngineBuild::Unknown);
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

    type AliasedList = Vec<u32>;

    #[derive(Default, TypeUuid, Reflect)]
    #[uuid = "c41d7a2e-58b3-4f19-9e06-2d8a6b3f7c15"]
    #[allow(unused)]
    struct WithLists {
        direct: Vec<f32>,
        aliased: AliasedList,
        nested: Vec<Vec<i64>>,
        plain: f32,
    }

    fn list_element(reg: &TypeRegistry, type_uuid: Uuid) -> Uuid {
        let Some(TypeInfo::List(info)) = reg.type_info_by_id(type_uuid) else {
            panic!("expected list info for {type_uuid}");
        };
        info.element.type_uuid
    }

    #[test]
    fn derive_registers_list_info_for_direct_vec_fields() {
        let mut reg = empty_registry();
        WithLists::register(&mut reg);

        assert_eq!(
            list_element(&reg, Vec::<f32>::type_uuid()),
            f32::type_uuid()
        );
        let TypeInfo::Struct(info) = reg.type_info::<WithLists>().unwrap() else {
            panic!("expected struct info");
        };
        assert_eq!(
            info.field("direct").unwrap().type_uuid,
            Vec::<f32>::type_uuid()
        );
    }

    #[test]
    fn derive_registers_list_info_for_aliased_vec_fields() {
        let mut reg = empty_registry();
        WithLists::register(&mut reg);

        assert_eq!(
            list_element(&reg, AliasedList::type_uuid()),
            u32::type_uuid()
        );
    }

    #[test]
    fn derive_registers_list_info_for_nested_vec_fields() {
        let mut reg = empty_registry();
        WithLists::register(&mut reg);

        assert_eq!(
            list_element(&reg, Vec::<Vec<i64>>::type_uuid()),
            Vec::<i64>::type_uuid()
        );
        assert_eq!(
            list_element(&reg, Vec::<i64>::type_uuid()),
            i64::type_uuid()
        );
    }

    #[test]
    fn derive_registers_defaults_for_list_types() {
        let mut reg = empty_registry();
        WithLists::register(&mut reg);

        // The inspector appends a list's element type's default, so a list of
        // lists can grow only when the inner list type has one.
        for list in [
            Vec::<f32>::type_uuid(),
            Vec::<Vec<i64>>::type_uuid(),
            Vec::<i64>::type_uuid(),
        ] {
            let default = reg.trait_meta::<ReflectDefault>(list).unwrap().default();
            assert_eq!(default.uuid(), list);
            assert!(default.as_list().unwrap().is_empty());
        }
    }

    #[test]
    fn derive_registers_no_list_info_for_plain_fields() {
        let mut reg = empty_registry();
        WithLists::register(&mut reg);

        assert!(reg.type_info::<f32>().is_none());
    }

    #[test]
    fn plugin_pass_keeps_host_owned_list_info() {
        let mut reg = empty_registry();
        reg.meta_field_list::<f32>();
        // Mark the host registration so a replacement would show: a fresh
        // registration has only the trait metadata it registers itself.
        reg.types
            .get_mut(&Vec::<f32>::type_uuid())
            .unwrap()
            .trait_meta
            .insert(
                ReflectGenericFloat::type_uuid(),
                Box::new(<ReflectGenericFloat as TraitMetaFrom<f32>>::trait_meta()),
            );

        reg.begin_plugin_load();
        reg.meta_field_list::<f32>();
        reg.meta_field_list::<u32>();

        let host = reg.type_registration::<Vec<f32>>().unwrap();
        assert!(host
            .trait_meta
            .contains_key(&ReflectGenericFloat::type_uuid()));
        assert_eq!(reg.identities.types[&Vec::<f32>::type_uuid()].1, 0);
        assert_eq!(
            list_element(&reg, Vec::<u32>::type_uuid()),
            u32::type_uuid()
        );
        assert_eq!(reg.identities.types[&Vec::<u32>::type_uuid()].1, 1);
    }

    #[derive(TypeUuid)]
    #[uuid = "3b0c2f5e-8d4a-4f6e-9a1b-7c5d2e8f1a40"]
    struct DupA;

    #[derive(TypeUuid)]
    #[uuid = "3b0c2f5e-8d4a-4f6e-9a1b-7c5d2e8f1a40"]
    struct DupB;

    #[derive(Default, TypeUuid, Reflect)]
    #[allow(unused)]
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
