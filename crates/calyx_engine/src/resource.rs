use crate::background::Background;
use crate::core::{Ref, Time};
use crate::input::InputState;
use crate::net::Network;
use crate::physics::PhysicsConfiguration;
use crate::runtime::HostCommands;
pub use calyx_engine_derive::Resource;
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Marker trait for values stored inside a [`ResourceMap`].
pub trait Resource: Any {}

impl dyn Resource {
    /// Returns `true` when this resource is a `T`.
    pub fn is<T: Resource>(&self) -> bool {
        let resource: &dyn Any = self;
        resource.is::<T>()
    }

    /// Downcasts this resource to `T` by shared reference.
    pub fn downcast_ref<T: Resource>(&self) -> Option<&T> {
        let resource: &dyn Any = self;
        resource.downcast_ref()
    }

    /// Downcasts this resource to `T` by mutable reference.
    pub fn downcast_mut<T: Resource>(&mut self) -> Option<&mut T> {
        let resource: &mut dyn Any = self;
        resource.downcast_mut()
    }
}

/// Type-indexed runtime resource storage used by the engine loop.
pub struct ResourceMap {
    inner: HashMap<TypeId, Box<dyn Resource>>,
}

impl Default for ResourceMap {
    fn default() -> Self {
        Self::new()
    }
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
        resources.insert_default::<InputState>();
        resources.insert_default::<HostCommands>();
        resources
    }

    #[inline]
    /// Inserts `resource`, replacing any existing value of the same type.
    pub fn insert<T: Resource>(&mut self, resource: T) {
        self.inner.insert(TypeId::of::<T>(), Box::new(resource));
    }

    #[inline]
    /// Inserts `T::default()`.
    pub fn insert_default<T: Resource + Default>(&mut self) {
        self.insert(T::default());
    }

    #[inline]
    /// Removes and returns the resource of type `T`.
    pub fn remove<T: Resource>(&mut self) -> Option<T> {
        let resource: Box<dyn Any> = self.inner.remove(&TypeId::of::<T>())?;
        resource.downcast().ok().map(|resource| *resource)
    }

    #[inline]
    /// Returns the resource of type `T`.
    pub fn resource<T: Resource>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|r| r.downcast_ref())
    }

    #[inline]
    /// Returns the mutable resource of type `T`.
    pub fn resource_mut<T: Resource>(&mut self) -> Option<&mut T> {
        self.inner
            .get_mut(&TypeId::of::<T>())
            .and_then(|r| r.downcast_mut())
    }

    #[inline]
    /// Returns mutable references to two distinct resources when both exist.
    pub fn resource_pair_mut<T1: Resource, T2: Resource>(&mut self) -> Option<(&mut T1, &mut T2)> {
        let id1 = TypeId::of::<T1>();
        let id2 = TypeId::of::<T2>();
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

    /// Returns the persistent input state resource.
    #[inline]
    pub fn input(&self) -> &InputState {
        self.resource::<InputState>().unwrap()
    }

    /// Returns the persistent input state resource mutably.
    #[inline]
    pub fn input_mut(&mut self) -> &mut InputState {
        self.resource_mut::<InputState>().unwrap()
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

    /// Returns the queue of commands for the game's host: see
    /// [`HostCommand`](crate::runtime::HostCommand).
    #[inline]
    pub fn host_commands_mut(&mut self) -> &mut HostCommands {
        self.resource_mut::<HostCommands>().unwrap()
    }
}

#[cfg(test)]
mod tests;
