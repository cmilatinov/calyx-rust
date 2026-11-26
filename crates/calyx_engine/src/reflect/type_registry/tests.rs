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
