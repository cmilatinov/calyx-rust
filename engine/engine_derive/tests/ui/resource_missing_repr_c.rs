#[derive(engine_derive::Resource)]
struct NoRepr {
    value: u32,
}

#[derive(engine_derive::Resource)]
#[repr(align(8))]
struct OtherRepr {
    value: u32,
}

fn main() {}
