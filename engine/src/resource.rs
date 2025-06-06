use crate::background::Background;
use crate::core::Time;
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
        resources.insert_default::<Background>();
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

    impl_getter!(mut time, Time);
    impl_getter!(mut background, Background);
}
