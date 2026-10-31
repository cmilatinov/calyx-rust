//! The tone-mapping pass: the chain's display transform.

use super::{PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct TonemapUniform {
    exposure_scale: f32,
    preset: u32,
    _padding: [u32; 2],
}

/// Applies exposure and the tone-mapping curve, from linear HDR to
/// display-linear color. Alpha passes through. It runs every frame.
pub struct TonemapPass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl TonemapPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/tonemap";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("tonemap_uniform_buffer"),
                    size: size_of::<TonemapUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        Ok(Self {
            shader,
            uniform_buffer,
            bind_group: Default::default(),
        })
    }
}

impl PostProcessPass for TonemapPass {
    fn name(&self) -> &str {
        "tonemap"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Tonemap
    }

    fn enabled(&self, _settings: &PostProcessSettings) -> bool {
        true
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let uniform = TonemapUniform {
            exposure_scale: settings.tonemapping.exposure_scale(),
            preset: settings.tonemapping.preset.shader_index(),
            _padding: Default::default(),
        };
        context.render_context.queue().write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
    }

    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo) {
        let shader = self.shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        ) else {
            return;
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = context.render_context.device();
        let uniform_buffer = &self.uniform_buffer;
        let bind_group =
            self.bind_group
                .get_or_create((layout.clone(), io.input.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("tonemap_bind_group"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&io.input.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                        ],
                    })
                });
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "Tonemap");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
    }
}
