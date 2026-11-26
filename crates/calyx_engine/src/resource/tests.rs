use super::{Resource, ResourceMap};
use crate as calyx_engine;
use crate::core::Ref;
use crate::utils::TypeUuid;

#[derive(Default, Resource, TypeUuid)]
#[uuid = "e8adeabf-1129-4d04-9a25-aa7f0d7c2bb5"]
struct Counter(u32);

#[derive(Default, Resource, TypeUuid)]
#[uuid = "a436a4c3-3a6d-49e8-a5ed-9820e73691e5"]
struct Flag(bool);

fn map_with_counter(n: u32) -> ResourceMap {
    let mut m = ResourceMap {
        inner: Default::default(),
    };
    m.insert(Counter(n));
    m
}

#[test]
fn insert_and_get() {
    let m = map_with_counter(42);
    assert_eq!(m.resource::<Counter>().unwrap().0, 42);
}

#[test]
fn missing_resource_returns_none() {
    let m = map_with_counter(0);
    assert!(m.resource::<Flag>().is_none());
}

#[test]
fn resource_mut_modifies() {
    let mut m = map_with_counter(1);
    m.resource_mut::<Counter>().unwrap().0 = 99;
    assert_eq!(m.resource::<Counter>().unwrap().0, 99);
}

#[test]
fn insert_default() {
    let mut m = ResourceMap {
        inner: Default::default(),
    };
    m.insert_default::<Counter>();
    assert_eq!(m.resource::<Counter>().unwrap().0, 0);
}

#[test]
fn resource_pair_mut() {
    let mut m = ResourceMap {
        inner: Default::default(),
    };
    m.insert(Counter(1));
    m.insert(Flag(false));
    let (c, f) = m.resource_pair_mut::<Counter, Flag>().unwrap();
    c.0 = 10;
    f.0 = true;
    assert_eq!(m.resource::<Counter>().unwrap().0, 10);
    assert!(m.resource::<Flag>().unwrap().0);
}

#[test]
fn new_registers_builtin_resources() {
    let resources = ResourceMap::new();
    assert!(resources.resource::<crate::core::Time>().is_some());
    assert!(resources.resource::<crate::net::Network>().is_some());
    assert!(resources
        .resource::<crate::physics::PhysicsConfiguration>()
        .is_some());
    assert!(resources
        .resource::<crate::core::Ref<crate::background::Background>>()
        .is_some());
    assert!(resources
        .resource::<crate::runtime::HostCommands>()
        .is_some());
}

// Two resources named `Probe`, which the `TypeUuid` derive gives the same
// type UUID since neither has a `#[uuid]` attribute.
mod first {
    use crate as calyx_engine;
    use crate::resource::Resource;
    use crate::utils::TypeUuid;

    #[derive(Resource, TypeUuid)]
    pub struct Probe(pub u32);
}

mod second {
    use crate as calyx_engine;
    use crate::resource::Resource;
    use crate::utils::TypeUuid;

    #[derive(Resource, TypeUuid)]
    pub struct Probe(pub String);
}

#[test]
fn same_named_resources_are_kept_apart() {
    assert_eq!(first::Probe::type_uuid(), second::Probe::type_uuid());
    let mut m = ResourceMap {
        inner: Default::default(),
    };
    m.insert(first::Probe(4));
    assert!(m.resource::<second::Probe>().is_none());
    assert!(m.resource_mut::<second::Probe>().is_none());
    assert!(m.remove::<second::Probe>().is_none());

    m.insert(second::Probe("probe".into()));
    let (first, second) = m
        .resource_pair_mut::<first::Probe, second::Probe>()
        .unwrap();
    first.0 += 1;
    second.0.push('!');
    assert_eq!(m.remove::<first::Probe>().map(|p| p.0), Some(5));
    assert_eq!(m.resource::<second::Probe>().unwrap().0, "probe!");
}

#[test]
fn a_resource_downcasts_only_to_its_own_type() {
    let mut resource: Box<dyn Resource> = Box::new(first::Probe(1));
    assert!(resource.is::<first::Probe>());
    assert!(!resource.is::<second::Probe>());
    assert!(resource.downcast_ref::<second::Probe>().is_none());
    assert!(resource.downcast_mut::<second::Probe>().is_none());
    resource.downcast_mut::<first::Probe>().unwrap().0 = 2;
    assert_eq!(resource.downcast_ref::<first::Probe>().unwrap().0, 2);
}

#[test]
fn resource_lookup_works_for_ref_wrappers() {
    let counter = Ref::new(Counter(5));
    let mut m = ResourceMap {
        inner: Default::default(),
    };
    m.insert(counter.clone());

    let restored = m.resource::<Ref<Counter>>().unwrap();
    assert_eq!(restored.ptr_id(), counter.ptr_id());
    assert_ne!(Counter::type_uuid(), Ref::<Counter>::type_uuid());
}
