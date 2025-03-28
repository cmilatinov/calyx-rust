use crate::reflect::TypeName;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};
use uuid::Uuid;

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

    pub fn read(&self) -> RwLockReadGuard<T> {
        self.inner.read().unwrap()
    }

    pub fn write(&self) -> RwLockWriteGuard<T> {
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

pub struct ReadOnlyRef<T: ?Sized> {
    inner: Ref<T>,
}

impl<T: ?Sized> ReadOnlyRef<T> {
    pub fn from_ref(inner: Ref<T>) -> Self {
        Self { inner }
    }

    pub fn read(&self) -> RwLockReadGuard<T> {
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
