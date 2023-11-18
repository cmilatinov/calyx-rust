use std::fmt::{Debug, Formatter};
use std::ops::Deref;
use std::sync::{Arc, RwLock};

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

pub struct OptionRef<T: ?Sized>(pub Option<Ref<T>>);

impl<T> OptionRef<T> {
    pub fn new(value: T) -> Self {
        Self(Some(Ref::<T>(Arc::new(RwLock::new(value)))))
    }
}

impl<T: ?Sized> OptionRef<T> {
    pub fn from_ref(reference: Ref<T>) -> Self {
        Self(Some(reference))
    }
    pub fn from_opt_ref(opt: Option<Ref<T>>) -> Self {
        Self(opt)
    }
}

impl<T> Default for OptionRef<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> Clone for OptionRef<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Debug> Debug for OptionRef<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<T: ?Sized> Deref for OptionRef<T> {
    type Target = Option<Ref<T>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
