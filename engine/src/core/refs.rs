use std::fmt::Debug;
use std::ops::Deref;
use std::sync::{Arc, RwLock};

#[derive(Default, Debug)]
pub struct Ref<T: ?Sized>(pub(crate) Arc<RwLock<T>>);

impl<T: ?Sized> Ref<T> {
    pub fn from_arc(value: Arc<RwLock<T>>) -> Ref<T> {
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

impl<T: ?Sized> Deref for Ref<T> {
    type Target = Arc<RwLock<T>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub type OptionRef<T> = Option<Ref<T>>;
