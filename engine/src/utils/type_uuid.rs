use uuid::Uuid;

pub trait TypeUuid {
    const UUID: &'static [u8; 16];
}

pub trait TypeUuidDynamic: TypeUuid {
    fn uuid_bytes() -> &'static [u8; 16] {
        Self::UUID
    }

    fn uuid() -> Uuid {
        Uuid::from_bytes(*Self::UUID)
    }
}
