#[derive(calyx_engine_derive::Reflect)]
#[reflect(Default = 1)]
struct BadTrait {
    value: u32,
}

#[derive(calyx_engine_derive::Reflect)]
struct BadFieldAttr {
    #[reflect_attr(a::b)]
    value: u32,
}

fn main() {}
