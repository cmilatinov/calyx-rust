struct Ctx;

#[derive(engine_derive::DeserializeWithContext)]
struct MissingContext {
    value: u32,
}

#[derive(engine_derive::DeserializeWithContext)]
#[context(Ctx)]
struct Tuple(u32);

#[derive(engine_derive::DeserializeWithContext)]
#[context(Ctx)]
struct Unit;

#[derive(engine_derive::DeserializeWithContext)]
#[context(Ctx)]
union NotSupported {
    a: u32,
}

#[derive(engine_derive::DeserializeWithContext)]
#[context = 1]
struct BadContext {
    value: u32,
}

fn main() {}
