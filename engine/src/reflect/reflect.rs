use std::any::Any;
use std::fmt::{Debug, Formatter};

use crate::reflect::type_registry::TypeRegistry;

use crate::utils::{TypeUuid, TypeUuidDynamic};
pub use engine_derive::Reflect;

pub trait TypeName {
    fn type_name() -> &'static str;
    fn type_name_short() -> &'static str;
}

pub trait TypeNameDynamic {
    fn type_name(&self) -> &'static str;
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

pub trait Reflect: TypeUuidDynamic + TypeNameDynamic + Any + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_reflect(&self) -> &dyn Reflect;
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
    fn assign(&mut self, value: Box<dyn Reflect>) -> bool;
}

impl dyn Reflect {
    pub fn is<T: Reflect + TypeUuid>(&self) -> bool {
        self.uuid() == T::type_uuid()
    }
    pub fn downcast_ref<T: Reflect + TypeUuid>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(&*(self as *const dyn Reflect as *const T)) }
        } else {
            None
        }
    }
    pub fn downcast_mut<T: Reflect + TypeUuid>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(&mut *(self as *mut dyn Reflect as *mut T)) }
        } else {
            None
        }
    }
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

pub trait ReflectedType {
    fn register(registry: &mut TypeRegistry);
}
