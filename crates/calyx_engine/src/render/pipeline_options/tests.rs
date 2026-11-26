use super::*;

#[test]
fn fullscreen_builder_matches_the_equivalent_pipeline_options() {
    let target = RenderUtils::color_alpha_blending(wgpu::TextureFormat::Rgba16Float);
    let depth = wgpu::DepthStencilState {
        depth_write_enabled: false,
        ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
    };
    let built = PipelineOptions::fullscreen_builder()
        .samples(4)
        .target(target.clone())
        .depth_stencil(depth.clone())
        .build();
    let expected = PipelineOptions::builder()
        .cull_mode(None)
        .vertex_layouts(vec![])
        .fragment_targets(vec![Some(target)])
        .depth_stencil(Some(depth))
        .samples(4)
        .build();
    assert_eq!(built, expected);
}

#[test]
fn fullscreen_builder_defaults_to_no_depth_culling_or_vertex_layouts() {
    let options = PipelineOptions::fullscreen_builder().build();
    assert_eq!(options.depth_stencil, None);
    assert_eq!(options.cull_mode, None);
    assert!(options.vertex_layouts.is_empty());
    assert!(options.fragment_targets.is_empty());
    assert_eq!(options.samples, 1);
}
