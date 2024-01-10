use crate as engine;
use sha1::Digest;
use uuid::Uuid;

pub use engine_derive::{reflect_trait, TypeUuid};

pub trait TypeUuid {
    const UUID: &'static [u8; 16];
    fn type_uuid() -> Uuid {
        Uuid::from_bytes(*Self::UUID)
    }
}

#[reflect_trait]
pub trait TypeUuidDynamic {
    fn uuid_bytes(&self) -> &'static [u8; 16];
    fn uuid(&self) -> Uuid;
}

impl<T: TypeUuid> TypeUuidDynamic for T {
    fn uuid_bytes(&self) -> &'static [u8; 16] {
        Self::UUID
    }

    fn uuid(&self) -> Uuid {
        Uuid::from_bytes(*Self::UUID)
    }
}

pub fn uuid_from_str(value: &str) -> Uuid {
    let mut hasher = sha1::Sha1::new();
    hasher.update(value.as_bytes());
    let hash = hasher.finalize();
    let mut bytes: [u8; 16] = [0; 16];
    bytes.copy_from_slice(&hash.as_slice()[0..16]);
    Uuid::from_bytes(bytes)
}
