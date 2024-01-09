use crate as engine;
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
