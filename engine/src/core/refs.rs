use crate::reflect::TypeName;
use crate::resource::Resource;
use crate::utils::{TypeUuid, TypeUuidDynamic};
use sha1::Digest;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use uuid::Uuid;

const REF_WRAPPER_UUID: [u8; 16] = [
    0x0C, 0xFC, 0xB5, 0xED, 0xA9, 0xF5, 0x41, 0xEC, 0x87, 0x4B, 0x24, 0xCD, 0x3F, 0x01, 0xB1, 0x2A,
];
const READ_ONLY_REF_WRAPPER_UUID: [u8; 16] = [
    0x17, 0xEF, 0x5A, 0xE1, 0x2C, 0xD0, 0x42, 0x9F, 0x84, 0x52, 0xA0, 0x21, 0x65, 0x55, 0xB0, 0x6A,
];

fn wrapper_type_uuid(wrapper_uuid: &[u8; 16], inner_uuid: Uuid) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(wrapper_uuid);
    hasher.update(inner_uuid.as_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}

#[repr(C)]
pub struct Ref<T: ?Sized> {
    pub(crate) id: Uuid,
    pub(crate) inner: Arc<RwLock<T>>,
}

impl<T> Ref<T> {
    pub fn from_id_value(id: Uuid, value: T) -> Self {
        Self {
            id,
            inner: Arc::new(RwLock::new(value)),
        }
    }

    pub fn new(value: T) -> Self {
        Self {
            id: Uuid::nil(),
            inner: Arc::new(RwLock::new(value)),
        }
    }

    pub fn new_cyclic<F: FnOnce(WeakRef<T>) -> T>(data_fn: F) -> Self {
        Self {
            id: Uuid::nil(),
            inner: Arc::new_cyclic(|weak| {
                RwLock::new(data_fn(WeakRef {
                    id: Uuid::nil(),
                    inner: weak.clone(),
                }))
            }),
        }
    }

    pub unsafe fn from_raw(ptr: *const RwLock<T>) -> Self {
        Self {
            id: Uuid::nil(),
            inner: Arc::from_raw(ptr),
        }
    }
}

impl<T: ?Sized> Ref<T> {
    pub fn downgrade(&self) -> WeakRef<T> {
        WeakRef::new(self)
    }

    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read().unwrap()
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.inner.write().unwrap()
    }

    pub fn readonly(&self) -> ReadOnlyRef<T> {
        ReadOnlyRef::from_ref(self.clone())
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn ptr_id(&self) -> usize {
        &*self.inner as *const _ as *const () as usize
    }
}

impl<T: ?Sized> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            inner: self.inner.clone(),
        }
    }
}

impl<T: Debug> Debug for Ref<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ref({:?})", &self.read())
    }
}

impl<T: TypeName> TypeName for Ref<T> {
    fn type_name() -> &'static str {
        std::any::type_name::<Ref<T>>()
    }

    fn type_name_short() -> &'static str {
        T::type_name()
    }
}

impl<T: TypeUuid> TypeUuid for Ref<T> {
    const UUID: &'static [u8; 16] = &[0; 16];

    fn type_uuid() -> Uuid {
        wrapper_type_uuid(&REF_WRAPPER_UUID, T::type_uuid())
    }
}

impl<T: Resource> TypeUuidDynamic for Ref<T> {
    fn uuid_bytes(&self) -> &'static [u8; 16] {
        &REF_WRAPPER_UUID
    }

    fn uuid(&self) -> Uuid {
        wrapper_type_uuid(&REF_WRAPPER_UUID, self.read().uuid())
    }
}

impl<T: Resource> Resource for Ref<T> {}

#[repr(C)]
pub struct ReadOnlyRef<T: ?Sized> {
    inner: Ref<T>,
}

impl<T: ?Sized> ReadOnlyRef<T> {
    pub fn from_ref(inner: Ref<T>) -> Self {
        Self { inner }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.inner.read()
    }

    pub fn id(&self) -> Uuid {
        self.inner.id
    }

    pub fn ptr_id(&self) -> usize {
        self.inner.ptr_id()
    }
}

impl<T: ?Sized + Debug> Debug for ReadOnlyRef<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReadOnlyRef({:?})", &self.read())
    }
}

impl<T: ?Sized + TypeName> TypeName for ReadOnlyRef<T> {
    fn type_name() -> &'static str {
        std::any::type_name::<ReadOnlyRef<T>>()
    }

    fn type_name_short() -> &'static str {
        T::type_name()
    }
}

impl<T: ?Sized> Clone for ReadOnlyRef<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: TypeUuid> TypeUuid for ReadOnlyRef<T> {
    const UUID: &'static [u8; 16] = &[0; 16];

    fn type_uuid() -> Uuid {
        wrapper_type_uuid(&READ_ONLY_REF_WRAPPER_UUID, T::type_uuid())
    }
}

impl<T: Resource> TypeUuidDynamic for ReadOnlyRef<T> {
    fn uuid_bytes(&self) -> &'static [u8; 16] {
        &READ_ONLY_REF_WRAPPER_UUID
    }

    fn uuid(&self) -> Uuid {
        wrapper_type_uuid(&READ_ONLY_REF_WRAPPER_UUID, self.read().uuid())
    }
}

impl<T: Resource> Resource for ReadOnlyRef<T> {}

#[repr(C)]
pub struct WeakRef<T: ?Sized> {
    id: Uuid,
    inner: Weak<RwLock<T>>,
}

impl<T: ?Sized> WeakRef<T> {
    pub fn new(value_ref: &Ref<T>) -> Self {
        Self {
            id: value_ref.id,
            inner: Arc::downgrade(&value_ref.inner),
        }
    }

    pub fn upgrade(&self) -> Option<Ref<T>> {
        self.inner.upgrade().map(|arc| Ref {
            id: self.id,
            inner: arc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadOnlyRef, Ref, WeakRef};
    use crate as engine;
    use crate::resource::Resource;
    use crate::utils::TypeUuid;

    #[derive(Default, Resource, TypeUuid)]
    #[uuid = "f13a97c5-8d80-42d0-ae95-f4f0bd42d390"]
    #[repr(C)]
    struct DummyResource(u32);

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
}
