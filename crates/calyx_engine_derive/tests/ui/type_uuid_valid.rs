mod calyx_engine {
    pub mod utils {
        pub use uuid::Uuid;

        pub trait TypeUuid {
            fn type_uuid() -> Uuid;
        }
    }
}

use calyx_engine::utils::TypeUuid;

#[derive(calyx_engine_derive::TypeUuid)]
#[uuid = "6f1c2d3e-4a5b-4c6d-8e7f-0a1b2c3d4e5f"]
struct Explicit;

fn main() {
    assert_eq!(Explicit::type_uuid().as_bytes()[0], 0x6f);
}
