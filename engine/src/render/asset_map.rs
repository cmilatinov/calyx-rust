use crate::core::Ref;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

pub(crate) struct AssetMap<T> {
    pub refs: HashMap<usize, Ref<T>>,
}

impl<T> Deref for AssetMap<T> {
    type Target = HashMap<usize, Ref<T>>;

    fn deref(&self) -> &Self::Target {
        &self.refs
    }
}

impl<T> DerefMut for AssetMap<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.refs
    }
}

impl<T> Default for AssetMap<T> {
    fn default() -> Self {
        Self {
            refs: Default::default(),
        }
    }
}

impl<T> AssetMap<T> {
    pub fn get(&self, id: usize) -> &Ref<T> {
        self.refs.get(&id).unwrap()
    }

    pub fn lock_read(&self) -> HashMap<usize, RwLockReadGuard<T>> {
        self.refs.iter().map(|(id, r)| (*id, r.read())).collect()
    }

    pub fn lock_write(&self) -> HashMap<usize, RwLockWriteGuard<T>> {
        self.refs.iter().map(|(id, r)| (*id, r.write())).collect()
    }
}