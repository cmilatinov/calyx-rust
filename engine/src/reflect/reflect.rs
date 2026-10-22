use std::any::Any;
use std::fmt::{Debug, Formatter};

use crate::reflect::type_registry::TypeRegistry;

use crate::utils::{TypeUuid, TypeUuidDynamic};
pub use engine_derive::Reflect;

/// Static type-name access used by reflection metadata.
pub trait TypeName {
    /// Fully qualified Rust type name.
    fn type_name() -> &'static str;
    /// Short display name for the type.
    fn type_name_short() -> &'static str;
}

/// Object-safe type-name access for reflected values.
pub trait TypeNameDynamic {
    /// Fully qualified Rust type name.
    fn type_name(&self) -> &'static str;
    /// Short display name for the type.
    fn type_name_short(&self) -> &'static str;
}

impl<T: TypeName> TypeNameDynamic for T {
    fn type_name(&self) -> &'static str {
        Self::type_name()
    }
    fn type_name_short(&self) -> &'static str {
        Self::type_name_short()
    }
}

/// Base trait for runtime-reflected values.
pub trait Reflect: TypeUuidDynamic + TypeNameDynamic + Any + Send + Sync {
    /// Returns this value as [`Any`].
    fn as_any(&self) -> &dyn Any;
    /// Returns this value as mutable [`Any`].
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Returns this value as [`Reflect`].
    fn as_reflect(&self) -> &dyn Reflect;
    /// Returns this value as mutable [`Reflect`].
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect;
    /// Converts this boxed reflected value into [`Any`].
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    /// Assigns `value` into `self` when the concrete types match.
    fn assign(&mut self, value: Box<dyn Reflect>) -> bool;
}

impl dyn Reflect {
    /// Returns `true` when this reflected value is a `T`.
    pub fn is<T: Reflect + TypeUuid>(&self) -> bool {
        self.uuid() == T::type_uuid()
    }
    /// Downcasts this reflected value by shared reference.
    pub fn downcast_ref<T: Reflect + TypeUuid>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Reflect as *const T)) }
        } else {
            None
        }
    }
    /// Downcasts this reflected value by mutable reference.
    pub fn downcast_mut<T: Reflect + TypeUuid>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Reflect as *mut T)) }
        } else {
            None
        }
    }
    /// Downcasts this boxed reflected value.
    pub fn downcast<T: Reflect + TypeUuid>(self: Box<Self>) -> Result<Box<T>, Box<dyn Reflect>> {
        if self.is::<T>() {
            let raw = Box::into_raw(self);
            unsafe { Ok(Box::from_raw(raw as *mut T)) }
        } else {
            Err(self)
        }
    }
}

impl Debug for dyn Reflect {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.type_name_short())
    }
}

/// Trait implemented by types that can register reflection metadata.
pub trait ReflectedType {
    /// Registers the type and any trait metadata into `registry`.
    fn register(registry: &mut TypeRegistry);
}
