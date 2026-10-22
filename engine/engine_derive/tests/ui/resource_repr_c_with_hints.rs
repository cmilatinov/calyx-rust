mod engine {
    pub mod resource {
        pub trait Resource {}
    }
}

fn assert_resource<T: engine::resource::Resource>() {}

#[derive(engine_derive::Resource)]
#[repr(C, align(16))]
struct Aligned {
    value: u32,
}

#[derive(engine_derive::Resource)]
#[repr(align(8))]
#[repr(C)]
struct SplitRepr {
    value: u32,
}

fn main() {
    assert_resource::<Aligned>();
    assert_resource::<SplitRepr>();
}
