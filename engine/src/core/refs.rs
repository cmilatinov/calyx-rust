use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::reflect::TypeName;

pub struct Ref<T: ?Sized>(Arc<RwLock<T>>);

impl<T: ?Sized> Ref<T> {
    pub fn read(&self) -> RwLockReadGuard<T> {
        self.0.read().unwrap()
    }

    pub fn write(&self) -> RwLockWriteGuard<T> {
        self.0.write().unwrap()
    }

    pub fn id(&self) -> usize {
        &***self as *const _ as *const () as usize
    }
}

impl<T: ?Sized> From<&Ref<T>> for Ref<T> {
    fn from(value: &Ref<T>) -> Self {
        Ref::<T>(value.0.clone())
    }
}

impl<T: ?Sized> From<Arc<RwLock<T>>> for Ref<T> {
    fn from(value: Arc<RwLock<T>>) -> Self {
        Self(value)
    }
}
impl<T> Ref<T> {
    pub fn new(value: T) -> Self {
        Ref::<T>(Arc::new(RwLock::new(value)))
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Ref::from(self)
    }
}

impl<T: Debug> Debug for Ref<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ref({:?})", self.0.read().unwrap())
    }
}

impl<T: ?Sized> Deref for Ref<T> {
    type Target = Arc<RwLock<T>>;
    fn deref(&self) -> &Self::Target {
        &self.0
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
