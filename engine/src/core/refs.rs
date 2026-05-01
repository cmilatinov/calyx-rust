use crate::reflect::TypeName;
use crate::resource::Resource;
use crate::utils::{uuid_from_str, TypeUuid};
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use uuid::Uuid;

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

impl<T: Resource + ?Sized> TypeUuid for Ref<T> {
    const UUID: &'static [u8; 16] = &[0; 16];

    fn type_uuid() -> Uuid {
        uuid_from_str(std::any::type_name::<Self>())
    }
}

impl<T: Resource + ?Sized> Resource for Ref<T> {
    fn resource_uuid(&self) -> Uuid {
        Self::type_uuid()
    }
}

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

impl<T: Resource + ?Sized> TypeUuid for ReadOnlyRef<T> {
    const UUID: &'static [u8; 16] = &[0; 16];

    fn type_uuid() -> Uuid {
        uuid_from_str(std::any::type_name::<Self>())
    }
}

impl<T: Resource + ?Sized> Resource for ReadOnlyRef<T> {
    fn resource_uuid(&self) -> Uuid {
        Self::type_uuid()
    }
}

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
    use super::{Ref, WeakRef};

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
}
