use crate::utils::TypeUuidDynamic;
use std::any::Any;

/// Allows you to cast structs implementing the Reflect trait
/// into a specific trait if they implement such a trait
///
/// The registry keys trait metadata by type UUID, and downcasts it through
/// [`Any`].
pub trait TraitMeta: Any + TypeUuidDynamic + Send + Sync {}

/// Allows creation of a TraitMeta object (ex: ReflectDefault)
/// from its original struct type
pub trait TraitMetaFrom<T> {
    /// Builds the reflected trait metadata wrapper for `T`.
    fn trait_meta() -> Self;
}
