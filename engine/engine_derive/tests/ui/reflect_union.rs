#[derive(engine_derive::Reflect)]
#[repr(C)]
union NotSupported {
    a: u32,
    b: f32,
}

fn main() {}
