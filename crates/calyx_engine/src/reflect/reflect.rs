use std::any::Any;
use std::fmt::{Debug, Formatter};

use crate::reflect::type_registry::TypeRegistry;

use crate::utils::TypeUuidDynamic;
pub use calyx_engine_derive::Reflect;

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
    /// Returns this value as an enum when it is one, which
    /// `#[derive(Reflect)]` makes every enum it derives.
    fn as_enum(&self) -> Option<&dyn ReflectEnum> {
        None
    }
    /// Returns this value as a mutable enum when it is one.
    fn as_enum_mut(&mut self) -> Option<&mut dyn ReflectEnum> {
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

/// Type-erased access to a reflected enum's current variant.
///
/// [`crate::reflect::EnumInfo`] lists the variants; this trait reads which
/// one a value holds and switches it to a unit variant by name.
/// `#[derive(Reflect)]` implements it for every enum it derives.
pub trait ReflectEnum {
    /// The name of the variant this value holds, as declared.
    fn variant_name(&self) -> &'static str;
    /// Sets this value to the unit variant `name`. Returns `false`, leaving
    /// the value as it was, when the enum has no variant `name` or that
    /// variant carries data.
    fn set_unit_variant(&mut self, name: &str) -> bool;
}

impl dyn Reflect {
    /// Returns `true` when this reflected value is a `T`.
    ///
    /// Compares `TypeId`s, so two types that share a name or a type UUID
    /// never pass for each other.
    pub fn is<T: Reflect>(&self) -> bool {
        let value: &dyn Any = self;
        value.is::<T>()
    }
    /// Downcasts this reflected value by shared reference.
    pub fn downcast_ref<T: Reflect>(&self) -> Option<&T> {
        let value: &dyn Any = self;
        value.downcast_ref()
    }
    /// Downcasts this reflected value by mutable reference.
    pub fn downcast_mut<T: Reflect>(&mut self) -> Option<&mut T> {
        let value: &mut dyn Any = self;
        value.downcast_mut()
    }
    /// Downcasts this boxed reflected value, handing it back when it is not a
    /// `T`.
    pub fn downcast<T: Reflect>(self: Box<Self>) -> Result<Box<T>, Box<dyn Reflect>> {
        if self.is::<T>() {
            let value: Box<dyn Any> = self;
            Ok(value
                .downcast()
                .unwrap_or_else(|_| unreachable!("the value was checked to be a T")))
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

#[cfg(test)]
mod tests;
