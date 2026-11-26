use super::{ReadOnlyRef, Ref, WeakRef};
use crate as calyx_engine;
use crate::resource::Resource;
use crate::utils::TypeUuid;

#[derive(Default, Resource, TypeUuid)]
#[uuid = "f13a97c5-8d80-42d0-ae95-f4f0bd42d390"]
struct DummyResource;

#[test]
fn read_write() {
    let r: Ref<i32> = Ref::new(42);
    assert_eq!(*r.read(), 42);
    *r.write() = 99;
    assert_eq!(*r.read(), 99);
}

#[test]
fn clone_shares_data() {
    let a: Ref<i32> = Ref::new(1);
    let b = a.clone();
    *a.write() = 7;
    assert_eq!(*b.read(), 7);
}

#[test]
fn weak_upgrade_and_drop() {
    let r: Ref<i32> = Ref::new(5);
    let weak: WeakRef<i32> = r.downgrade();
    assert!(weak.upgrade().is_some());
    drop(r);
    assert!(weak.upgrade().is_none());
}

#[test]
fn readonly_ref() {
    let r: Ref<i32> = Ref::new(10);
    let ro = r.readonly();
    assert_eq!(*ro.read(), 10);
}

#[test]
fn ptr_id_same_for_clones() {
    let a: Ref<i32> = Ref::new(0);
    let b = a.clone();
    assert_eq!(a.ptr_id(), b.ptr_id());
}

#[test]
fn wrapper_type_uuids_are_stable_and_distinct() {
    assert_ne!(
        DummyResource::type_uuid(),
        Ref::<DummyResource>::type_uuid()
    );
    assert_ne!(
        Ref::<DummyResource>::type_uuid(),
        ReadOnlyRef::<DummyResource>::type_uuid()
    );
    assert_eq!(
        Ref::<DummyResource>::type_uuid(),
        Ref::<DummyResource>::type_uuid()
    );
}

#[derive(Default, Resource, TypeUuid)]
#[uuid = "0b4c0c1e-6a52-4a53-9d2b-3d5fd6f8c7a1"]
struct OtherResource;

#[test]
fn wrapper_uuids_derive_from_pointee() {
    use crate::utils::TypeUuidDynamic;
    let a = Ref::new(DummyResource);
    let b = Ref::new(OtherResource);
    assert_eq!(TypeUuidDynamic::uuid(&a), Ref::<DummyResource>::type_uuid());
    assert_ne!(TypeUuidDynamic::uuid(&a), TypeUuidDynamic::uuid(&b));
    let (ra, rb) = (a.readonly(), b.readonly());
    assert_eq!(
        TypeUuidDynamic::uuid(&ra),
        ReadOnlyRef::<DummyResource>::type_uuid()
    );
    assert_ne!(TypeUuidDynamic::uuid(&ra), TypeUuidDynamic::uuid(&rb));
}
