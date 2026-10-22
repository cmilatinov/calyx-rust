#[derive(engine_derive::Reflect)]
#[repr(C)]
#[reflect(Default = 1)]
struct BadTrait {
    value: u32,
}

#[derive(engine_derive::Reflect)]
#[repr(C)]
struct BadFieldAttr {
    #[reflect_attr(a::b)]
    value: u32,
}

fn main() {}
