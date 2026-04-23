use crate::background::Background;
use crate::core::{Ref, Time};
use crate::net::Network;
use downcast_rs::{impl_downcast, Downcast};
pub use engine_derive::Resource;
use paste::paste;
use std::any::{Any, TypeId};
use std::collections::HashMap;

pub trait Resource: Downcast + Any + 'static {}
impl_downcast!(Resource);

pub struct ResourceMap {
    inner: HashMap<TypeId, Box<dyn Resource>>,
}

macro_rules! impl_getter {
    ($ident:ident, $ty:ty) => {
        #[inline]
        pub fn $ident(&self) -> &$ty {
            self.resource::<$ty>().unwrap()
        }
    };
    (mut $ident:ident, $ty:ty) => {
        paste! {
            impl_getter!($ident, $ty);

            #[inline]
            pub fn [<$ident _mut>](&mut self) -> &mut $ty {
                self.resource_mut::<$ty>().unwrap()
            }
        }
    };
}

impl ResourceMap {
    pub fn new() -> Self {
        let mut resources = Self {
            inner: Default::default(),
        };
        resources.insert_default::<Time>();
        resources.insert::<Ref<Background>>(Background::new());
        resources.insert_default::<Network>();
        resources
    }

    #[inline]
    pub fn insert<T: Resource>(&mut self, resource: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(resource));
    }

    #[inline]
    pub fn insert_default<T: Resource + Default>(&mut self) {
        self.insert(T::default());
    }

    #[inline]
    pub fn resource<T: Resource>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|r| r.downcast_ref())
    }

    #[inline]
    pub fn resource_mut<T: Resource>(&mut self) -> Option<&mut T> {
        self.inner
            .get_mut(&TypeId::of::<T>())
            .and_then(|r| r.downcast_mut())
    }

    #[inline]
    pub fn resource_pair_mut<T1: Resource, T2: Resource>(&mut self) -> Option<(&mut T1, &mut T2)> {
        match self
            .inner
            .get_disjoint_mut([&TypeId::of::<T1>(), &TypeId::of::<T2>()])
        {
            [Some(value1), Some(value2)] => {
                if let (Some(value1), Some(value2)) =
                    (value1.downcast_mut::<T1>(), value2.downcast_mut::<T2>())
                {
                    Some((value1, value2))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    impl_getter!(mut time, Time);
    impl_getter!(mut background, Ref<Background>);
    impl_getter!(mut network, Network);
}

#[cfg(test)]
mod tests {
    use super::{Resource, ResourceMap};

    #[derive(Default)]
    struct Counter(u32);
    impl Resource for Counter {}

    #[derive(Default)]
    struct Flag(bool);
    impl Resource for Flag {}

    fn map_with_counter(n: u32) -> ResourceMap {
        let mut m = ResourceMap { inner: Default::default() };
        m.insert(Counter(n));
        m
    }

    #[test]
    fn insert_and_get() {
        let m = map_with_counter(42);
        assert_eq!(m.resource::<Counter>().unwrap().0, 42);
    }

    #[test]
    fn missing_resource_returns_none() {
        let m = map_with_counter(0);
        assert!(m.resource::<Flag>().is_none());
    }

    #[test]
    fn resource_mut_modifies() {
        let mut m = map_with_counter(1);
        m.resource_mut::<Counter>().unwrap().0 = 99;
        assert_eq!(m.resource::<Counter>().unwrap().0, 99);
    }

    #[test]
    fn insert_default() {
        let mut m = ResourceMap { inner: Default::default() };
        m.insert_default::<Counter>();
        assert_eq!(m.resource::<Counter>().unwrap().0, 0);
    }

    #[test]
    fn resource_pair_mut() {
        let mut m = ResourceMap { inner: Default::default() };
        m.insert(Counter(1));
        m.insert(Flag(false));
        let (c, f) = m.resource_pair_mut::<Counter, Flag>().unwrap();
        c.0 = 10;
        f.0 = true;
        assert_eq!(m.resource::<Counter>().unwrap().0, 10);
        assert!(m.resource::<Flag>().unwrap().0);
    }
}
