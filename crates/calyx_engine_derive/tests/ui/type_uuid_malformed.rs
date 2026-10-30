mod calyx_engine {
    pub mod utils {
        pub use uuid::Uuid;

        pub trait TypeUuid {
            fn type_uuid() -> Uuid;
        }
    }
}

#[derive(calyx_engine_derive::TypeUuid)]
#[uuid = "not-a-uuid"]
struct Malformed;

fn main() {}
