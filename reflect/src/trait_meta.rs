use downcast_rs::{Downcast, impl_downcast};

/// Allows you to cast structs implementing the Reflect trait
/// into a specific trait if they implement such a trait
pub trait TraitMeta: Downcast + Send + Sync {}
impl_downcast!(TraitMeta);

/// Allows creation of a TraitMeta object (ex: ReflectDefault)
/// from its original struct type
pub trait TraitMetaFrom<T> {
    fn trait_meta() -> Self;
}
