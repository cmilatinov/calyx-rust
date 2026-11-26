use super::ParticleRenderer;
use crate::component::ParticleBlendMode;
use crate::render::Shader;
use crate::test_support::TestAssets;
use egui_wgpu::wgpu::TextureFormat;

#[test]
fn particle_renderer_builds_a_pipeline_per_blend_mode() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let renderer =
        ParticleRenderer::new(&read_only_context).expect("particle renderer should load");

    let alpha = renderer.pipeline(
        ParticleBlendMode::Alpha,
        1,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    );
    let additive = renderer.pipeline(
        ParticleBlendMode::Additive,
        1,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    );
    let multisampled = renderer.pipeline(
        ParticleBlendMode::Alpha,
        4,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    );

    assert_ne!(alpha, additive);
    assert_ne!(alpha, multisampled);
    assert_eq!(
        alpha,
        renderer.pipeline(
            ParticleBlendMode::Alpha,
            1,
            TextureFormat::Rgba16Float,
            TextureFormat::Depth32Float,
        )
    );
    assert_eq!(renderer.shader.read().pipeline_count(), 3);
}

#[test]
fn particle_renderer_uses_the_reloaded_shader_pipeline() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let read_only_context = context.lock_read();
    let renderer =
        ParticleRenderer::new(&read_only_context).expect("particle renderer should load");
    let before = renderer.pipeline(
        ParticleBlendMode::Alpha,
        1,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    );

    let registry = read_only_context.registries.assets.read();
    let id = registry
        .asset_id("shaders/particles/particles")
        .expect("particle shader should have metadata");
    let reloaded = registry
        .reload_by_id::<Shader>(id)
        .expect("particle shader should reload");
    assert_eq!(reloaded.ptr_id(), renderer.shader.ptr_id());

    let after = renderer.pipeline(
        ParticleBlendMode::Alpha,
        1,
        TextureFormat::Rgba16Float,
        TextureFormat::Depth32Float,
    );
    assert_ne!(before, after);
    assert_eq!(
        after,
        reloaded
            .read()
            .pipeline(&ParticleRenderer::pipeline_options(
                ParticleBlendMode::Alpha,
                1,
                TextureFormat::Rgba16Float,
                TextureFormat::Depth32Float,
            ))
    );
    assert_eq!(reloaded.read().pipeline_count(), 1);
}
