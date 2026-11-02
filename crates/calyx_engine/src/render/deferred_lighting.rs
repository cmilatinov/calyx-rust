//! The deferred lighting pass: shades the opaque PBR surfaces of the
//! G-buffer into the scene's HDR target with the lighting the forward PBR
//! pass uses. The paths and their pass order are in
//! `docs/design/post-processing/architecture.md`.

use crate::assets::texture::Texture;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{
    CachedBindGroup, GBufferTargets, LightingBindGroups, PipelineOptions, RenderContext,
    RenderUtils, SceneRendererError, Shader,
};
use egui::Color32;
use egui_wgpu::wgpu;

/// The G-buffer's views the pass binds in group 1: albedo, normals,
/// material, emissive, then depth.
type GBufferBindGroupKey = (wgpu::BindGroupLayout, [wgpu::TextureView; 5]);

/// Shades the G-buffer into the scene target with a full-screen pass.
pub(crate) struct DeferredLightingRenderer {
    /// `shaders/deferred/lighting.wgsl`, at one sample.
    shader: Ref<Shader>,
    /// `shaders/deferred/lighting_msaa.wgsl`, above one sample: also writes
    /// the G-buffer's depth into the multisampled scene depth.
    msaa_shader: Ref<Shader>,
    gbuffer_bind_group: CachedBindGroup<GBufferBindGroupKey>,
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
        })
    }

    /// Drops the cached bind group, on a frame the deferred path does not
    /// run, so it holds no G-buffer view.
    pub fn release(&mut self) {
        self.gbuffer_bind_group.clear();
    }

    /// Clears `color` to `clear_color` and writes the lit color of every
    /// pixel the geometry pass drew into `gbuffer`, against `gbuffer_depth`,
    /// lit through `lighting`.
    ///
    /// At one sample the geometry pass drew `scene_depth` itself and the pass
    /// leaves it. Above one, `color` and `scene_depth` are multisampled: the
    /// pass writes each pixel's color and G-buffer depth into all of its
    /// samples and clears the scene depth elsewhere, so what draws after it
    /// tests against the deferred surfaces.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        render_context: &RenderContext,
        encoder: &mut wgpu::CommandEncoder,
        gbuffer: &GBufferTargets,
        gbuffer_depth: &Texture,
        lighting: &LightingBindGroups,
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
        let layout = shader.bind_group_layouts[1].clone();
        drop(shader);
        let [albedo, normals, material, emissive] = gbuffer.color_targets();
        let views = [
            albedo.view.clone(),
            normals.view.clone(),
            material.view.clone(),
            emissive.view.clone(),
            gbuffer_depth.view.clone(),
        ];
        let device = render_context.device();
        let gbuffer_bind_group =
            self.gbuffer_bind_group
                .get_or_create((layout.clone(), views.clone()), || {
                    let entries: Vec<wgpu::BindGroupEntry> = views
                        .iter()
                        .enumerate()
                        .map(|(binding, view)| wgpu::BindGroupEntry {
                            binding: binding as u32,
                            resource: wgpu::BindingResource::TextureView(view),
                        })
                        .collect();
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("deferred_lighting_gbuffer_bind_group"),
                        layout: &layout,
                        entries: &entries,
                    })
                });

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
        let Some(pipeline) = pipeline else {
            return;
        };
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &lighting.scene, &[]);
        pass.set_bind_group(1, gbuffer_bind_group, &[]);
        pass.set_bind_group(2, &lighting.lights, &[]);
        RenderUtils::draw_fullscreen(&mut pass);
    }
}
