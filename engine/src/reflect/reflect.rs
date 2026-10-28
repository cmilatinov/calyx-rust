use std::any::Any;
use std::fmt::{Debug, Formatter};

use crate::reflect::type_registry::TypeRegistry;

use crate::utils::TypeUuidDynamic;
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
mod tests {
    use super::{Reflect, TypeName};
    use crate as engine;
    use crate::utils::TypeUuid;
    use std::any::Any;

    /// Implements [`Reflect`] by hand, so the test types stay out of the
    /// inventory every [`crate::reflect::type_registry::TypeRegistry`] reads.
    macro_rules! reflect_by_hand {
        ($ty:ty) => {
            impl TypeName for $ty {
                fn type_name() -> &'static str {
                    std::any::type_name::<$ty>()
                }
                fn type_name_short() -> &'static str {
                    "Probe"
                }
            }

            impl Reflect for $ty {
                fn as_any(&self) -> &dyn Any {
                    self
                }
                fn as_any_mut(&mut self) -> &mut dyn Any {
                    self
                }
                fn as_reflect(&self) -> &dyn Reflect {
                    self
                }
                fn as_reflect_mut(&mut self) -> &mut dyn Reflect {
                    self
                }
                fn into_any(self: Box<Self>) -> Box<dyn Any> {
                    self
                }
                fn assign(&mut self, value: Box<dyn Reflect>) -> bool {
                    match value.downcast::<$ty>() {
                        Ok(value) => {
                            *self = *value;
                            true
                        }
                        Err(_) => false,
                    }
                }
            }
        };
    }

    // Two types named `Probe`. The `TypeUuid` derive names a type without a
    // `#[uuid]` attribute by its bare name, so they share a type UUID too.
    mod first {
        use crate as engine;
        use crate::utils::TypeUuid;

        #[derive(TypeUuid)]
        pub struct Probe(pub u32);
    }

    mod second {
        use crate as engine;
        use crate::utils::TypeUuid;

        #[derive(TypeUuid)]
        pub struct Probe(pub String);
    }

    reflect_by_hand!(first::Probe);
    reflect_by_hand!(second::Probe);

    #[derive(TypeUuid)]
    #[uuid = "4b8f0e57-1f0a-4e0e-9f3b-6d1e2c7a9b10"]
    struct Unrelated;

    #[test]
    fn same_named_types_share_a_type_uuid() {
        assert_eq!(first::Probe::type_uuid(), second::Probe::type_uuid());
        assert_ne!(first::Probe::type_uuid(), Unrelated::type_uuid());
    }

    #[test]
    fn downcast_ref_tells_same_named_types_apart() {
        let value: Box<dyn Reflect> = Box::new(first::Probe(7));
        assert_eq!(value.uuid(), second::Probe::type_uuid());
        assert!(value.is::<first::Probe>());
        assert!(!value.is::<second::Probe>());
        assert_eq!(value.downcast_ref::<first::Probe>().map(|p| p.0), Some(7));
        assert!(value.downcast_ref::<second::Probe>().is_none());
    }

    #[test]
    fn downcast_mut_tells_same_named_types_apart() {
        let mut value: Box<dyn Reflect> = Box::new(second::Probe("probe".into()));
        assert!(value.downcast_mut::<first::Probe>().is_none());
        value.downcast_mut::<second::Probe>().unwrap().0.push('!');
        assert_eq!(value.downcast_ref::<second::Probe>().unwrap().0, "probe!");
    }

    #[test]
    fn boxed_downcast_hands_back_a_same_named_type() {
        let value: Box<dyn Reflect> = Box::new(first::Probe(3));
        let Err(value) = value.downcast::<second::Probe>() else {
            panic!("a first::Probe is not a second::Probe");
        };
        assert_eq!(value.downcast::<first::Probe>().map(|p| p.0).ok(), Some(3));
    }

    #[test]
    fn assign_refuses_a_same_named_type() {
        let mut target = first::Probe(1);
        assert!(!target.assign(Box::new(second::Probe("other".into()))));
        assert_eq!(target.0, 1);
        assert!(target.assign(Box::new(first::Probe(2))));
        assert_eq!(target.0, 2);
    }
}
