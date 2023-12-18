use uuid::Uuid;

pub use reflect_derive::TypeUuid;

pub trait TypeUuid {
    const UUID: &'static [u8; 16];
    fn type_uuid() -> Uuid {
        Uuid::from_bytes(*Self::UUID)
    }
}

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
