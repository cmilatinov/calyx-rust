use super::OutlineRenderer;
use crate::render::RenderUtils;
use crate::test_support::TestAssets;
use egui_wgpu::wgpu::TextureFormat;

#[test]
fn outline_renderer_loads_shader_and_builds_pipeline() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let renderer = OutlineRenderer::new(&read_only_context).expect("outline renderer should load");
    let shader = renderer.shader.read();
    let options = crate::render::PipelineOptions::fullscreen_builder()
        .target(RenderUtils::color_alpha_blending(
            TextureFormat::Rgba16Float,
        ))
        .build();
    let pipeline = shader.pipeline(&options);
    assert_eq!(shader.pipeline(&options), pipeline);
}
