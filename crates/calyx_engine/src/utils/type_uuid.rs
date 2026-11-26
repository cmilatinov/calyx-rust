use crate as calyx_engine;
use sha1::Digest;
pub use uuid::Uuid;

pub use calyx_engine_derive::{reflect_trait, TypeUuid};

/// Trait for types with a stable UUID.
pub trait TypeUuid {
    /// Returns this type's UUID.
    fn type_uuid() -> Uuid;
}

#[reflect_trait]
/// Object-safe UUID access used by reflection and resource systems.
pub trait TypeUuidDynamic {
    /// Returns the UUID for this value's concrete type.
    fn uuid(&self) -> Uuid;
}

impl<T: TypeUuid> TypeUuidDynamic for T {
    fn uuid(&self) -> Uuid {
        Self::type_uuid()
    }
}

/// Produces a deterministic UUID by hashing `value`.
pub fn uuid_from_str(value: &str) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let mut bytes: [u8; 16] = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests;
