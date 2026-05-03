use crate::background::Background;
use crate::core::{Ref, Time};
use crate::net::Network;
use crate::physics::PhysicsConfiguration;
use crate::utils::{TypeUuid, TypeUuidDynamic};
pub use engine_derive::Resource;
use std::collections::HashMap;
use uuid::Uuid;

/// Marker trait for values stored inside a [`ResourceMap`].
pub trait Resource: TypeUuidDynamic + 'static {}

impl dyn Resource {
    /// Returns `true` when this resource is a `T`.
    pub fn is<T: Resource + TypeUuid>(&self) -> bool {
        self.uuid() == T::type_uuid()
    }

    /// Downcasts this resource to `T` by shared reference.
    pub fn downcast_ref<T: Resource + TypeUuid>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Resource as *const T)) }
        } else {
            None
        }
    }

    /// Downcasts this resource to `T` by mutable reference.
    pub fn downcast_mut<T: Resource + TypeUuid>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Resource as *mut T)) }
        } else {
            None
        }
    }
}

/// Type-indexed runtime resource storage used by the engine loop.
pub struct ResourceMap {
    inner: HashMap<Uuid, Box<dyn Resource>>,
}

impl ResourceMap {
    /// Creates a resource map populated with the built-in engine resources.
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
    /// Inserts `resource`, replacing any existing value of the same type.
    pub fn insert<T: Resource + TypeUuid>(&mut self, resource: T) {
        self.inner.insert(T::type_uuid(), Box::new(resource));
    }

    #[inline]
    /// Inserts `T::default()`.
    pub fn insert_default<T: Resource + Default + TypeUuid>(&mut self) {
        self.insert(T::default());
    }

    #[inline]
    /// Removes and returns the resource of type `T`.
    pub fn remove<T: Resource + TypeUuid>(&mut self) -> Option<T> {
        let resource = self.inner.remove(&T::type_uuid())?;
        let raw = Box::into_raw(resource);
        unsafe { Some(*Box::from_raw(raw as *mut T)) }
    }

    #[inline]
    /// Returns the resource of type `T`.
    pub fn resource<T: Resource + TypeUuid>(&self) -> Option<&T> {
        self.inner
            .get(&T::type_uuid())
            .and_then(|r| r.downcast_ref())
    }

    #[inline]
    /// Returns the mutable resource of type `T`.
    pub fn resource_mut<T: Resource + TypeUuid>(&mut self) -> Option<&mut T> {
        self.inner
            .get_mut(&T::type_uuid())
            .and_then(|r| r.downcast_mut())
    }

    #[inline]
    /// Returns mutable references to two distinct resources when both exist.
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

    /// Returns the global time resource.
    #[inline]
    pub fn time(&self) -> &Time {
        self.resource::<Time>().unwrap()
    }

    /// Returns the global time resource mutably.
    #[inline]
    pub fn time_mut(&mut self) -> &mut Time {
        self.resource_mut::<Time>().unwrap()
    }

    /// Returns the background task resource.
    #[inline]
    pub fn background(&self) -> &Ref<Background> {
        self.resource::<Ref<Background>>().unwrap()
    }

    /// Returns the background task resource mutably.
    #[inline]
    pub fn background_mut(&mut self) -> &mut Ref<Background> {
        self.resource_mut::<Ref<Background>>().unwrap()
    }

    /// Returns the networking resource.
    #[inline]
    pub fn network(&self) -> &Network {
        self.resource::<Network>().unwrap()
    }

    /// Returns the networking resource mutably.
    #[inline]
    pub fn network_mut(&mut self) -> &mut Network {
        self.resource_mut::<Network>().unwrap()
    }

    /// Returns the physics configuration resource.
    #[inline]
    pub fn physics_configuration(&self) -> &PhysicsConfiguration {
        self.resource::<PhysicsConfiguration>().unwrap()
    }

    /// Returns the physics configuration resource mutably.
    #[inline]
    pub fn physics_configuration_mut(&mut self) -> &mut PhysicsConfiguration {
        self.resource_mut::<PhysicsConfiguration>().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::{Resource, ResourceMap};
    use crate as engine;
    use crate::core::Ref;
    use crate::utils::TypeUuid;

    #[derive(Default, Resource, TypeUuid)]
    #[uuid = "e8adeabf-1129-4d04-9a25-aa7f0d7c2bb5"]
    #[repr(C)]
    struct Counter(u32);

    #[derive(Default, Resource, TypeUuid)]
    #[uuid = "a436a4c3-3a6d-49e8-a5ed-9820e73691e5"]
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
        assert!(resources
            .resource::<crate::physics::PhysicsConfiguration>()
            .is_some());
        assert!(resources
            .resource::<crate::core::Ref<crate::background::Background>>()
            .is_some());
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
