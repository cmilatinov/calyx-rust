mod engine {
    pub mod utils {
        pub use uuid::Uuid;

        pub trait TypeUuid {
            fn type_uuid() -> Uuid;
        }
    }
}

#[derive(engine_derive::TypeUuid)]
#[uuid = 123]
struct NotAString;

#[derive(engine_derive::TypeUuid)]
#[uuid("6f1c2d3e-4a5b-4c6d-8e7f-0a1b2c3d4e5f")]
struct ListForm;

fn main() {}
