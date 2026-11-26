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
    /// Registers one enum variant with its attributes and field
    /// descriptors.
    pub fn variant(
        &mut self,
        name: &'static str,
        attrs: AttributeMap,
        fields: Vec<EnumVariantFieldInfo>,
    ) -> &mut EnumInfoBuilder<'a> {
        self.type_info.variants.push(EnumVariantInfo {
            name,
            attrs,
            fields,
        });
        self
    }
}

#[cfg(test)]
mod tests;
