include!("post_process_effect_valid.rs");

#[derive(calyx_engine_derive::PostProcessEffect)]
struct MissingBlend;

impl calyx_engine::component::Component for MissingBlend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
