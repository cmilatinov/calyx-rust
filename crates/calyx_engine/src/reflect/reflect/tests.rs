use super::{Reflect, TypeName};
use crate as calyx_engine;
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
    use crate as calyx_engine;
    use crate::utils::TypeUuid;

    #[derive(TypeUuid)]
    pub struct Probe(pub u32);
}

mod second {
    use crate as calyx_engine;
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
