//! The deferred lighting pass: shades the opaque PBR surfaces of the
//! G-buffer into the scene's HDR target through `pbr_lighting`, with SSAO's
//! occlusion on their ambient light. The pass order is in
//! `docs/design/post-processing/architecture.md`.

use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::postprocess::SsaoPass;
use crate::render::{
    CachedBindGroup, GBufferTargets, LightingBindGroups, PipelineOptions, RenderContext,
    RenderUtils, SceneRendererError, Shader,
};
use egui::Color32;
use egui_wgpu::wgpu;

/// The shader layouts and the views the pass binds in group 1: the
/// G-buffer's albedo, normals, material, emissive and depth, then SSAO's
/// occlusion.
type GBufferBindGroupKey = (u64, [wgpu::TextureView; 6]);

/// Shades the G-buffer into the scene target with a full-screen pass.
pub(crate) struct DeferredLightingRenderer {
    /// `shaders/deferred/lighting.wgsl`, at one sample.
    shader: Ref<Shader>,
    /// `shaders/deferred/lighting_msaa.wgsl`, above one sample: also writes
    /// the G-buffer's depth into the multisampled scene depth.
    msaa_shader: Ref<Shader>,
    gbuffer_bind_group: CachedBindGroup<GBufferBindGroupKey>,
    /// The occlusion bound while SSAO is off: one texel of visibility 1 at
    /// the sky's view depth, which no surface's upsample weighs, so every
    /// surface keeps its ambient light.
    unoccluded: Texture,
}

impl DeferredLightingRenderer {
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        Ok(Self {
            shader: SceneRendererError::load::<Shader>(context, "shaders/deferred/lighting")?,
            msaa_shader: SceneRendererError::load::<Shader>(
                context,
                "shaders/deferred/lighting_msaa",
            )?,
            gbuffer_bind_group: Default::default(),
            unoccluded: Self::unoccluded(context),
        })
    }

    /// The single texel of [`Self::unoccluded`]: visibility 1.0 and view
    /// depth 60000.0 (`SSAO_SKY_VIEW_Z`), as half floats.
    fn unoccluded(context: &ReadOnlyAssetContext) -> Texture {
        const TEXEL: [u16; 2] = [0x3c00, 0x7b53];
        let render_context = context.render_context.clone();
        let texture = SsaoPass::create_target(render_context.clone(), (1, 1), "ssao_unoccluded");
        render_context.queue().write_texture(
            texture.texture.as_image_copy(),
            bytemuck::cast_slice(&TEXEL),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: None,
                rows_per_image: None,
            },
            texture.descriptor.size,
        );
        texture
    }

    /// Clears `color` to `clear_color` and writes the lit color of every
    /// pixel the geometry pass drew into `gbuffer`, against `gbuffer_depth`,
    /// lit through `lighting`. `occlusion`, SSAO's blurred occlusion, dims
    /// their ambient and image-based light; without it nothing does but the
    /// materials' own occlusion.
    ///
    /// At one sample the geometry pass drew `scene_depth` itself and the pass
    /// leaves it. Above one, `color` and `scene_depth` are multisampled: the
    /// pass writes each pixel's color and G-buffer depth into all of its
    /// samples and clears the scene depth elsewhere, so what draws after it
    /// tests against the deferred surfaces.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        _render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        gbuffer: &GBufferTargets,
        gbuffer_depth: &Texture,
        lighting: &LightingBindGroups,
        occlusion: Option<&Texture>,
        color: &Texture,
        scene_depth: &Texture,
        clear_color: Color32,
    ) {
        let samples = color.descriptor.sample_count;
        let multisampled = samples > 1;
        let shader = if multisampled {
            self.msaa_shader.read()
        } else {
            self.shader.read()
        };
        let mut options = PipelineOptions::fullscreen_builder()
            .samples(samples)
            .target(RenderUtils::color_default(color.descriptor.format))
            .build();
        if multisampled {
            options.depth_stencil = Some(wgpu::DepthStencilState {
                depth_compare: wgpu::CompareFunction::Always,
                ..RenderUtils::depth_default(scene_depth.descriptor.format)
            });
        }
        let pipeline = shader.pipeline(&options);
        let [albedo, normals, material, emissive] = gbuffer.color_targets();
        let views = [
            albedo.view.clone(),
            normals.view.clone(),
            material.view.clone(),
            emissive.view.clone(),
            gbuffer_depth.view.clone(),
            occlusion.unwrap_or(&self.unoccluded).view.clone(),
        ];
        // Bound by name, so a reload that renumbers the G-buffer's bindings
        // rebuilds the group for its new layout.
        let gbuffer_bind_group = self
            .gbuffer_bind_group
            .get_or_try_create((shader.layout_generation(), views.clone()), || {
                let names = [
                    "albedo_texture",
                    "normals_texture",
                    "material_texture",
                    "emissive_texture",
                    "depth_texture",
                    "occlusion_texture",
                ];
                let resources = names
                    .iter()
                    .zip(&views)
                    .map(|(name, view)| (*name, wgpu::BindingResource::TextureView(view)))
                    .collect::<Vec<_>>();
                shader.create_bind_group(
                    1,
                    Some("deferred_lighting_gbuffer_bind_group"),
                    &resources,
                )
            })
            .cloned();
        drop(shader);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Deferred Lighting"),
            color_attachments: &[Some(RenderUtils::color_attachment(
                &color.view,
                clear_color,
            ))],
            depth_stencil_attachment: multisampled
                .then(|| RenderUtils::depth_stencil_attachment(&scene_depth.view, 1.0, None)),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let (Some(pipeline), Some(gbuffer_bind_group)) = (pipeline, gbuffer_bind_group) else {
            return;
        };
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &lighting.scene, &[]);
        pass.set_bind_group(1, &gbuffer_bind_group, &[]);
        pass.set_bind_group(2, &lighting.lights, &[]);
        RenderUtils::draw_fullscreen(&mut pass);
    }
}
