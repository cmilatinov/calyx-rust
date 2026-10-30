struct Ctx;

#[derive(calyx_engine_derive::DeserializeWithContext)]
struct MissingContext {
    value: u32,
}

#[derive(calyx_engine_derive::DeserializeWithContext)]
#[context(Ctx)]
struct Tuple(u32);

#[derive(calyx_engine_derive::DeserializeWithContext)]
#[context(Ctx)]
struct Unit;

#[derive(calyx_engine_derive::DeserializeWithContext)]
#[context(Ctx)]
union NotSupported {
    a: u32,
}

#[derive(calyx_engine_derive::DeserializeWithContext)]
#[context = 1]
struct BadContext {
    value: u32,
}

fn main() {}
