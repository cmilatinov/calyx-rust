use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::sync::{Arc, RwLock};

use crate::reflect::TypeName;

pub struct Ref<T: ?Sized>(Arc<RwLock<T>>);

impl<T: ?Sized> Ref<T> {
    pub fn from(reference: &Ref<T>) -> Self {
        Ref::<T>(reference.0.clone())
    }
    pub fn from_arc(value: Arc<RwLock<T>>) -> Self {
        Ref::<T>(value)
    }
}

impl<T> Ref<T> {
    pub fn new(value: T) -> Self {
        Ref::<T>(Arc::new(RwLock::new(value)))
    }
}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        Ref::from_arc(self.0.clone())
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
