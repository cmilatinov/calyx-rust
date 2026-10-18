mod engine {
    pub mod utils {
        pub trait TypeUuid {
            const UUID: &'static [u8; 16];
        }
    }
}

#[derive(engine_derive::TypeUuid)]
#[uuid = "not-a-uuid"]
struct Malformed;

fn main() {}
