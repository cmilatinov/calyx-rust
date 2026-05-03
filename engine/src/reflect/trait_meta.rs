use crate::utils::TypeUuidDynamic;

/// Allows you to cast structs implementing the Reflect trait
/// into a specific trait if they implement such a trait
pub trait TraitMeta: TypeUuidDynamic + Send + Sync {}

/// Allows creation of a TraitMeta object (ex: ReflectDefault)
/// from its original struct type
pub trait TraitMetaFrom<T> {
    /// Builds the reflected trait metadata wrapper for `T`.
    fn trait_meta() -> Self;
}
