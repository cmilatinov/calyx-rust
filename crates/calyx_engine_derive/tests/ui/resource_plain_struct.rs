mod calyx_engine {
    pub mod resource {
        pub trait Resource {}
    }
}

fn assert_resource<T: calyx_engine::resource::Resource>() {}

#[derive(calyx_engine_derive::Resource)]
struct Plain {
    value: u32,
}

#[derive(calyx_engine_derive::Resource)]
#[repr(align(16))]
struct Aligned {
    value: u32,
}

fn main() {
    assert_resource::<Plain>();
    assert_resource::<Aligned>();
}
