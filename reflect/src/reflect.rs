use crate::type_registry::TypeRegistry;
use std::any::{Any, TypeId};
use std::fmt::{Debug, Formatter};

pub trait Reflect: Any + Send + Sync {
    fn type_name(&self) -> &'static str;
    fn type_name_short(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_reflect(&self) -> &dyn Reflect;
    fn as_reflect_mut(&mut self) -> &mut dyn Reflect;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

impl dyn Reflect {
    pub fn is<T: Reflect>(&self) -> bool {
        self.type_id() == TypeId::of::<T>()
    }
    pub fn downcast_ref<T: Reflect>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
    pub fn downcast_mut<T: Reflect>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
    pub fn downcast<T: Reflect>(self: Box<Self>) -> Result<Box<T>, Box<dyn Reflect>> {
        if self.is::<T>() {
            Ok(self.into_any().downcast().unwrap())
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
