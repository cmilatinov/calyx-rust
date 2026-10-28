mod engine {
    pub mod resource {
        pub trait Resource {}
    }
}

fn assert_resource<T: engine::resource::Resource>() {}

#[derive(engine_derive::Resource)]
struct Plain {
    value: u32,
}

#[derive(engine_derive::Resource)]
#[repr(align(16))]
struct Aligned {
    value: u32,
}

fn main() {
    assert_resource::<Plain>();
    assert_resource::<Aligned>();
}
