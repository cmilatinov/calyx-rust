//! The FXAA pass: smooths aliased edges after tone mapping.

use super::{PassContext, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage};
use crate as calyx_engine;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use crate::utils::TypeUuid;
use egui_wgpu::wgpu;
use serde::{Deserialize, Serialize};

/// Screen-space anti-aliasing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "de6e6d51-2a88-47cf-b1cc-0cafe432a02e"]
#[reflect(Default)]
#[serde(default)]
pub struct AntiAliasingSettings {
    /// Whether FXAA runs after tone mapping.
    pub fxaa: bool,
    /// Minimum local contrast, relative to the brightest neighbour, that FXAA
    /// treats as an edge. Lower smooths more edges and costs more.
    #[reflect_attr(min = 0.063, max = 0.333, speed = 0.001)]
    pub edge_threshold: f32,
    /// How much sub-pixel aliasing FXAA removes, from 0 (none) to 1
    /// (softest).
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub subpixel: f32,
}

impl Default for AntiAliasingSettings {
    fn default() -> Self {
        // FXAA 3.11's default quality settings.
        Self {
            fxaa: false,
            edge_threshold: 0.166,
            subpixel: 0.75,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FxaaUniform {
    edge_threshold: f32,
    subpixel: f32,
    _padding: [f32; 2],
}

/// FXAA 3.11 Quality on display-linear color, in the `Ldr` stage. Alpha
/// passes through. It runs while [`AntiAliasingSettings::fxaa`] is on.
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
        settings.anti_aliasing.fxaa
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let uniform = FxaaUniform {
            edge_threshold: settings.anti_aliasing.edge_threshold,
            subpixel: settings.anti_aliasing.subpixel,
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
        let pipeline = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        );
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
    }
}
