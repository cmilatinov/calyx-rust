#[derive(engine_derive::Reflect)]
struct NumericGroup {
    #[reflect_attr(group = 1)]
    value: u32,
}

#[derive(engine_derive::Reflect)]
struct MissingGroupName {
    #[reflect_attr(group)]
    value: u32,
}

#[derive(engine_derive::Reflect)]
struct EmptyGroup {
    #[reflect_attr(group = "")]
    value: u32,
}

fn main() {}
