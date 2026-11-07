//! The FXAA pass: smooths aliased edges after tone mapping.

use super::{
    NoPipeline, PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage,
};
use crate::component::{ComponentAntiAliasing, FxaaQuality};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FxaaUniform {
    pub(crate) edge_threshold: f32,
    pub(crate) subpixel: f32,
    pub(crate) edge_threshold_min: f32,
    pub(crate) search_steps: u32,
    pub(crate) search_step_sizes: [[f32; 4]; 3],
}

impl FxaaUniform {
    pub(crate) fn new(settings: &ComponentAntiAliasing) -> Self {
        let steps = settings.quality.search_step_sizes();
        let mut sizes = [0.0; FxaaQuality::MAX_SEARCH_STEPS];
        sizes[..steps.len()].copy_from_slice(steps);
        Self {
            edge_threshold: settings.edge_threshold,
            subpixel: settings.subpixel,
            edge_threshold_min: settings.quality.edge_threshold_min(),
            search_steps: steps.len() as u32,
            search_step_sizes: std::array::from_fn(|row| {
                std::array::from_fn(|column| sizes[row * 4 + column])
            }),
        }
    }
}

/// FXAA 3.11 Quality on display-linear color, in the `Ldr` stage, at the
/// component's [`FxaaQuality`] preset, which the uniform carries. Alpha
/// passes through. It runs while the settings hold an
/// [`ComponentAntiAliasing`](crate::component::ComponentAntiAliasing).
pub struct FxaaPass {
    shader: Ref<Shader>,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
}

impl FxaaPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/fxaa";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let device = context.render_context.device();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fxaa_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fxaa_uniform_buffer"),
            size: size_of::<FxaaUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            shader,
            sampler,
            uniform_buffer,
            bind_group: Default::default(),
        })
    }
}

impl PostProcessPass for FxaaPass {
    fn name(&self) -> &str {
        "fxaa"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Ldr
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.get::<ComponentAntiAliasing>().is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(anti_aliasing) = settings.get::<ComponentAntiAliasing>() else {
            return;
        };
        let uniform = FxaaUniform::new(anti_aliasing);
        context.render_context.queue().write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
    }

    fn record(
        &mut self,
        context: &PassContext,
        encoder: &mut wgpu::CommandEncoder,
        io: PassIo,
    ) -> Result<(), NoPipeline> {
        let shader = self.shader.read();
        let Some(pipeline) = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        ) else {
            return Err(NoPipeline::of(&shader));
        };
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let device = context.render_context.device();
        let sampler = &self.sampler;
        let uniform_buffer = &self.uniform_buffer;
        let bind_group =
            self.bind_group
                .get_or_create((layout.clone(), io.input.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("fxaa_bind_group"),
                        layout: &layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&io.input.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(sampler),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: uniform_buffer.as_entire_binding(),
                            },
                        ],
                    })
                });
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "FXAA");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }
}
