use crate::background::Background;
use crate::core::{Ref, Time};
use crate::net::Network;
use crate::physics::PhysicsConfiguration;
use crate::utils::TypeUuid;
pub use engine_derive::Resource;
use paste::paste;
use std::collections::HashMap;
use uuid::Uuid;

pub trait Resource: 'static {
    fn resource_uuid(&self) -> Uuid;
}

impl dyn Resource {
    pub fn is<T: Resource + TypeUuid>(&self) -> bool {
        self.resource_uuid() == T::type_uuid()
    }

    pub fn downcast_ref<T: Resource + TypeUuid>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Resource as *const T)) }
        } else {
            None
        }
    }

    pub fn downcast_mut<T: Resource + TypeUuid>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Resource as *mut T)) }
        } else {
            None
        }
    }
}

pub struct ResourceMap {
    inner: HashMap<Uuid, Box<dyn Resource>>,
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
        resources.insert_default::<PhysicsConfiguration>();
        resources
    }

    #[inline]
    pub fn insert<T: Resource + TypeUuid>(&mut self, resource: T) {
        self.inner.insert(T::type_uuid(), Box::new(resource));
    }

    #[inline]
    pub fn insert_default<T: Resource + Default + TypeUuid>(&mut self) {
        self.insert(T::default());
    }

    #[inline]
    pub fn remove<T: Resource + TypeUuid>(&mut self) -> Option<T> {
        let resource = self.inner.remove(&T::type_uuid())?;
        let raw = Box::into_raw(resource);
        unsafe { Some(*Box::from_raw(raw as *mut T)) }
    }

    #[inline]
    pub fn resource<T: Resource + TypeUuid>(&self) -> Option<&T> {
        self.inner.get(&T::type_uuid()).and_then(|r| r.downcast_ref())
    }

    #[inline]
    pub fn resource_mut<T: Resource + TypeUuid>(&mut self) -> Option<&mut T> {
        self.inner
            .get_mut(&T::type_uuid())
            .and_then(|r| r.downcast_mut())
    }

    #[inline]
    pub fn resource_pair_mut<T1: Resource + TypeUuid, T2: Resource + TypeUuid>(
        &mut self,
    ) -> Option<(&mut T1, &mut T2)> {
        let id1 = T1::type_uuid();
        let id2 = T2::type_uuid();
        match self.inner.get_disjoint_mut([&id1, &id2]) {
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
    impl_getter!(mut physics_configuration, PhysicsConfiguration);
}

#[cfg(test)]
mod tests {
    use crate as engine;
    use super::{Resource, ResourceMap};
    use crate::core::Ref;
    use crate::utils::TypeUuid;

    #[derive(Default, Resource)]
    #[repr(C)]
    struct Counter(u32);

    #[derive(Default, Resource)]
    #[repr(C)]
    struct Flag(bool);

    fn map_with_counter(n: u32) -> ResourceMap {
        let mut m = ResourceMap {
            inner: Default::default(),
        };
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
        let mut m = ResourceMap {
            inner: Default::default(),
        };
        m.insert_default::<Counter>();
        assert_eq!(m.resource::<Counter>().unwrap().0, 0);
    }

    #[test]
    fn resource_pair_mut() {
        let mut m = ResourceMap {
            inner: Default::default(),
        };
        m.insert(Counter(1));
        m.insert(Flag(false));
        let (c, f) = m.resource_pair_mut::<Counter, Flag>().unwrap();
        c.0 = 10;
        f.0 = true;
        assert_eq!(m.resource::<Counter>().unwrap().0, 10);
        assert!(m.resource::<Flag>().unwrap().0);
    }

    #[test]
    fn new_registers_builtin_resources() {
        let resources = ResourceMap::new();
        assert!(resources.resource::<crate::core::Time>().is_some());
        assert!(resources.resource::<crate::net::Network>().is_some());
        assert!(resources.resource::<crate::physics::PhysicsConfiguration>().is_some());
        assert!(resources.resource::<crate::core::Ref<crate::background::Background>>().is_some());
    }

    #[test]
    fn resource_lookup_works_for_ref_wrappers() {
        let counter = Ref::new(Counter(5));
        let mut m = ResourceMap {
            inner: Default::default(),
        };
        m.insert(counter.clone());

        let restored = m.resource::<Ref<Counter>>().unwrap();
        assert_eq!(restored.ptr_id(), counter.ptr_id());
        assert_ne!(Counter::type_uuid(), Ref::<Counter>::type_uuid());
    }
}
