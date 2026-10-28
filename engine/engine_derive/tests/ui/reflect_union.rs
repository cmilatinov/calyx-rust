#[derive(engine_derive::Reflect)]
union NotSupported {
    a: u32,
    b: f32,
}

fn main() {}
