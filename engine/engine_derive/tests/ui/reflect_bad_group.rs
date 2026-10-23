#[derive(engine_derive::Reflect)]
#[repr(C)]
struct NumericGroup {
    #[reflect_attr(group = 1)]
    value: u32,
}

#[derive(engine_derive::Reflect)]
#[repr(C)]
struct MissingGroupName {
    #[reflect_attr(group)]
    value: u32,
}

#[derive(engine_derive::Reflect)]
#[repr(C)]
struct EmptyGroup {
    #[reflect_attr(group = "")]
    value: u32,
}

fn main() {}
