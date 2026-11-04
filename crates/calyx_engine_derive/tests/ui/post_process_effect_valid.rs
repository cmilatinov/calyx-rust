// Minimal downstream API: no engine traits are imported into the derive's scope.
mod calyx_engine {
    pub mod component {
        pub trait Component: std::any::Any {
            fn as_any(&self) -> &dyn std::any::Any;
        }

        pub trait PostProcessEffect: Component {
            fn copy_from(&mut self, source: &dyn PostProcessEffect);
            fn blend_into(&mut self, target: &dyn PostProcessEffect, factor: f32);
            fn reset_neutral(&mut self) -> bool;
        }
    }

    pub mod render {
        pub mod postprocess {
            pub trait BlendEffect {
                fn copy_in_place(&mut self, source: &Self);
                fn blend_in_place(&mut self, target: &Self, factor: f32);
                fn reset_neutral_in_place(&mut self) -> bool;
            }
        }
    }
}

#[derive(calyx_engine_derive::PostProcessEffect)]
struct Settings<T, const N: usize = 1>
where
    T: Copy + 'static,
{
    value: T,
    last_factor: f32,
}

impl<T: Copy + 'static, const N: usize> calyx_engine::component::Component for Settings<T, N> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<T: Copy + 'static, const N: usize> calyx_engine::render::postprocess::BlendEffect for Settings<T, N> {
    fn copy_in_place(&mut self, source: &Self) {
        self.value = source.value;
    }

    fn blend_in_place(&mut self, target: &Self, factor: f32) {
        self.value = target.value;
        self.last_factor = factor;
    }

    fn reset_neutral_in_place(&mut self) -> bool {
        false
    }
}

fn main() {
    let source = Settings::<_, 3> { value: 12_u32, last_factor: 0.0 };
    let mut destination = Settings::<_, 3> { value: 0_u32, last_factor: 0.0 };
    let erased: &mut dyn calyx_engine::component::PostProcessEffect = &mut destination;
    erased.copy_from(&source);
    erased.blend_into(&source, 2.0);
    assert!(!erased.reset_neutral());
    assert_eq!(destination.value, 12);
    assert_eq!(destination.last_factor, 1.0);
}
