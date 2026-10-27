//! The tone-mapping pass: the chain's display transform.

use super::{PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, SceneRendererError, Shader};
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
        let pipeline = shader.pipeline(&fullscreen_pipeline_options(io.output.descriptor.format));
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
        let mut render_pass = begin_fullscreen_pass(encoder, &io.output.view, "Tonemap");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Pipeline options of a full-screen pass writing one `format` target:
/// no vertex buffers, no depth, no blending, no culling.
pub fn fullscreen_pipeline_options(format: wgpu::TextureFormat) -> PipelineOptions {
    PipelineOptions::builder()
        .cull_mode(None)
        .fragment_targets(vec![Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })])
        .depth_stencil(None)
        .vertex_layouts(vec![])
        .build()
}

/// Begins a pass that overwrites every pixel of `output`. It clears rather
/// than loads, since the old contents are never read.
pub fn begin_fullscreen_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    output: &wgpu::TextureView,
    label: &str,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}
