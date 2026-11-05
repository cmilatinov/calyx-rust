//! The fog pass: fades the scene towards a fog colour by distance from the
//! camera, on linear HDR color before tone mapping.

use super::{
    CameraPlanes, NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass,
    PostProcessSettings, PostProcessStage, SceneInput,
};
use crate::component::ComponentFog;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FogUniform {
    /// Linear HDR fog colour in rgb, opacity in a.
    color: [f32; 4],
    camera: CameraPlanes,
    mode: u32,
    start: f32,
    end: f32,
    density: f32,
}

/// Distance fog in the `Hdr` stage. It reads the scene depth, rebuilds each
/// pixel's distance from the camera along its ray, and mixes the color
/// towards the fog colour by the fog's amount at that distance. The
/// background lies at the far plane, so it takes the fog of that distance.
/// It runs while the settings hold a
/// [`ComponentFog`](crate::component::ComponentFog).
pub struct FogPass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::TextureView)>,
}

impl FogPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/fog";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fog_uniform_buffer"),
                    size: size_of::<FogUniform>() as u64,
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

impl PostProcessPass for FogPass {
    fn name(&self) -> &str {
        "fog"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Hdr
    }

    /// The scene depth: opaque geometry, forward meshes and the particles
    /// that write depth fog by their own distance.
    fn inputs(&self) -> PassInputs {
        SceneInput::Depth.into()
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.get::<ComponentFog>().is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(fog) = settings.get::<ComponentFog>() else {
            return;
        };
        let color = egui::Rgba::from(fog.color);
        let intensity = fog.intensity.max(0.0);
        let uniform = FogUniform {
            color: [
                color.r() * intensity,
                color.g() * intensity,
                color.b() * intensity,
                fog.opacity,
            ],
            camera: context.camera,
            mode: fog.mode.shader_index(),
            start: fog.start,
            end: fog.end,
            density: fog.density,
        };
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
        let depth = io
            .depth
            .expect("the chain gives depth to a pass that asks for it");
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
        let uniform_buffer = &self.uniform_buffer;
        let bind_group = self.bind_group.get_or_create(
            (layout.clone(), io.input.view.clone(), depth.view.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fog_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "Fog");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }

    fn release_targets(&mut self) {
        self.bind_group.clear();
    }
}

#[cfg(test)]
mod tests;
