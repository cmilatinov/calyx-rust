use crate as engine;
use sha1::Digest;
use uuid::Uuid;

pub use engine_derive::{reflect_trait, TypeUuid};

pub trait TypeUuid {
    fn uuid_bytes() -> [u8; 16];

    fn type_uuid() -> Uuid {
        Uuid::from_bytes(Self::uuid_bytes())
    }
}

#[reflect_trait]
pub trait TypeUuidDynamic {
    fn uuid_bytes(&self) -> [u8; 16];
    fn uuid(&self) -> Uuid;
}

impl<T: TypeUuid> TypeUuidDynamic for T {
    fn uuid_bytes(&self) -> [u8; 16] {
        Self::uuid_bytes()
    }

    fn uuid(&self) -> Uuid {
        Self::type_uuid()
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

#[cfg(test)]
mod tests {
    use crate as engine;
    use super::{uuid_from_str, TypeUuid};
    use uuid::Uuid;

    #[derive(TypeUuid)]
    struct Plain;

    mod left {
        use crate as engine;
        use super::TypeUuid;

        #[derive(TypeUuid)]
        pub struct Collision;
    }

    mod right {
        use crate as engine;
        use super::TypeUuid;

        #[derive(TypeUuid)]
        pub struct Collision;
    }

    #[test]
    fn deterministic() {
        assert_eq!(uuid_from_str("hello"), uuid_from_str("hello"));
    }

    #[test]
    fn different_inputs_differ() {
        assert_ne!(uuid_from_str("foo"), uuid_from_str("bar"));
    }

    #[test]
    fn empty_string_is_stable() {
        let a = uuid_from_str("");
        let b = uuid_from_str("");
        assert_eq!(a, b);
    }

    #[test]
    fn derived_type_uuid_uses_fully_qualified_type_name() {
        assert_eq!(
            Plain::type_uuid(),
            uuid_from_str(std::any::type_name::<Plain>())
        );
    }

    #[test]
    fn derived_type_uuid_distinguishes_same_ident_in_different_modules() {
        assert_eq!(
            left::Collision::type_uuid(),
            uuid_from_str(std::any::type_name::<left::Collision>())
        );
        assert_eq!(
            right::Collision::type_uuid(),
            uuid_from_str(std::any::type_name::<right::Collision>())
        );
        assert_ne!(left::Collision::type_uuid(), right::Collision::type_uuid());
    }

    #[test]
    fn explicit_uuid_override_is_preserved() {
        #[derive(TypeUuid)]
        #[uuid = "d3a50f0b-0aa3-41ed-a4de-ff5f0d1740f8"]
        struct Explicit;

        assert_eq!(
            Explicit::type_uuid(),
            Uuid::parse_str("d3a50f0b-0aa3-41ed-a4de-ff5f0d1740f8").unwrap()
        );
    }
}
