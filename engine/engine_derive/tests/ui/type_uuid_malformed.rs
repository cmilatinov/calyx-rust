mod engine {
    pub mod utils {
        pub use uuid::Uuid;

        pub trait TypeUuid {
            fn type_uuid() -> Uuid;
        }
    }
}

#[derive(engine_derive::TypeUuid)]
#[uuid = "not-a-uuid"]
struct Malformed;

fn main() {}
