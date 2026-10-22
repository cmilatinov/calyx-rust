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
    /// Returns this value as a list when it is one, such as a `Vec`.
    fn as_list(&self) -> Option<&dyn ReflectList> {
        None
    }
    /// Returns this value as a mutable list when it is one, such as a `Vec`.
    fn as_list_mut(&mut self) -> Option<&mut dyn ReflectList> {
        None
    }
    /// Registers the container metadata this type needs when a reflected
    /// struct holds it in a field. `Vec<E>` registers its list info and
    /// default, then recurses into `E`; other types need nothing.
    ///
    /// The `Reflect` derive calls this for every reflected field. Dispatching
    /// on the resolved type means a field declared through a type alias of
    /// `Vec<E>` registers too.
    #[doc(hidden)]
    fn register_field_type(_registry: &mut TypeRegistry)
    where
        Self: Sized,
    {
    }
}

/// Type-erased access to a reflected list's elements.
///
/// [`crate::reflect::ListInfo`] describes the element type; this trait reads,
/// edits, adds and removes the elements themselves.
pub trait ReflectList {
    /// Number of elements.
    fn len(&self) -> usize;
    /// Returns `true` when the list holds no elements.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Element at `index`, or `None` past the end.
    fn element(&self, index: usize) -> Option<&(dyn Reflect + 'static)>;
    /// Mutable element at `index`, or `None` past the end.
    fn element_mut(&mut self, index: usize) -> Option<&mut (dyn Reflect + 'static)>;
    /// Appends `value`, handing it back when it is not of the element type.
    fn push(&mut self, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;
    /// Inserts `value` at `index`, shifting later elements up. Hands `value`
    /// back when it is not of the element type or `index` is past the end.
    fn insert(&mut self, index: usize, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>>;
    /// Removes and returns the element at `index`, or `None` past the end.
    fn remove(&mut self, index: usize) -> Option<Box<dyn Reflect>>;
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
