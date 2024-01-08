use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};
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
        reference.into()
    }
    pub fn from_opt_ref(opt: Option<Ref<T>>) -> Self {
        opt.into()
    }
}

impl<T: ?Sized> From<Option<Ref<T>>> for OptionRef<T> {
    fn from(value: Option<Ref<T>>) -> Self {
        Self(value)
    }
}

impl<T: ?Sized> From<Ref<T>> for OptionRef<T> {
    fn from(value: Ref<T>) -> Self {
        Self(Some(value))
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

impl<T: ?Sized> DerefMut for OptionRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct VecRef<T: ?Sized>(pub Vec<Ref<T>>);

impl<T: ?Sized> From<Vec<Ref<T>>> for VecRef<T> {
    fn from(value: Vec<Ref<T>>) -> Self {
        Self(value)
    }
}

impl<T> Default for VecRef<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T: Clone> Clone for VecRef<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Debug> Debug for VecRef<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<T: ?Sized> Deref for VecRef<T> {
    type Target = Vec<Ref<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for VecRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
