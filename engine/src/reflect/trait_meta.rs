use crate::utils::TypeUuidDynamic;
use downcast_rs::{impl_downcast, Downcast};

/// Allows you to cast structs implementing the Reflect trait
/// into a specific trait if they implement such a trait
pub trait TraitMeta: TypeUuidDynamic + Downcast + Send + Sync {}
impl_downcast!(TraitMeta);

/// Allows creation of a TraitMeta object (ex: ReflectDefault)
/// from its original struct type
pub trait TraitMetaFrom<T> {
    fn trait_meta() -> Self;
}
