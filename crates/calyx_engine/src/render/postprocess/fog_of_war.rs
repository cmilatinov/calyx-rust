//! The fog of war pass: darkens what a side cannot see, by the ground
//! position of each pixel's surface, on display-linear color after tone
//! mapping.

use super::{
    CameraPlanes, NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass,
    PostProcessSettings, PostProcessStage, SceneInput,
};
use crate::component::{ComponentFogOfWar, FogOfWarSight};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::buffer::ResizableBuffer;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct FogOfWarUniform {
    /// sRGB-encoded fog colour in rgb, opacity in a.
    color: [f32; 4],
    camera: CameraPlanes,
    view_to_world: [[f32; 4]; 4],
    viewer_count: u32,
    _pad: [u32; 3],
}

/// One viewer as the shader reads it: `Viewer` in
/// `shaders/postprocess/fog_of_war.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuViewer {
    position: [f32; 2],
    fade_start: f32,
    fade_end: f32,
    fade_curve: f32,
    shadow_jump: f32,
    first_ray: u32,
    ray_count: u32,
}

/// Packs `sight` into the shader's viewer table and the flat run of every
/// viewer's ray reaches, reusing the storage of `viewers` and `reach`.
pub(crate) fn pack_sight(
    sight: &FogOfWarSight,
    viewers: &mut Vec<GpuViewer>,
    reach: &mut Vec<f32>,
) {
    viewers.clear();
    reach.clear();
    for viewer in &sight.viewers {
        viewers.push(GpuViewer {
            position: [viewer.position.x, viewer.position.y],
            fade_start: viewer.fade_start,
            fade_end: viewer.fade_end,
            fade_curve: viewer.fade_curve,
            shadow_jump: viewer.shadow_jump,
            first_ray: reach.len() as u32,
            ray_count: viewer.reach.len() as u32,
        });
        reach.extend_from_slice(&viewer.reach);
    }
}

/// Fog of war in the `Ldr` stage, after the ink outlines and before FXAA,
/// which smooths its hard edges with the rest of the image. It reads the
/// scene depth, rebuilds each pixel's world position, and mixes the pixel
/// towards the fog colour by how little the side sees of its ground
/// position `(x, z)` (see [`ComponentFogOfWar`]). It runs while the
/// settings hold a [`ComponentFogOfWar`] that game code gave a sight.
pub struct FogOfWarPass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    viewer_buffer: ResizableBuffer,
    reach_buffer: ResizableBuffer,
    viewers: Vec<GpuViewer>,
    reach: Vec<f32>,
    #[allow(clippy::type_complexity)]
    bind_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::TextureView,
        wgpu::TextureView,
        u64,
        u64,
    )>,
}

impl FogOfWarPass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/fog_of_war";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("fog_of_war_uniform_buffer"),
                    size: size_of::<FogOfWarUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        let storage =
            || ResizableBuffer::new(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        Ok(Self {
            shader,
            uniform_buffer,
            viewer_buffer: storage(),
            reach_buffer: storage(),
            viewers: Vec::new(),
            reach: Vec::new(),
            bind_group: Default::default(),
        })
    }
}

impl PostProcessPass for FogOfWarPass {
    fn name(&self) -> &str {
        "fog_of_war"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Ldr
    }

    /// The scene depth, to place each pixel's surface in the world.
    fn inputs(&self) -> PassInputs {
        SceneInput::Depth.into()
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings
            .get::<ComponentFogOfWar>()
            .is_some_and(|fog| fog.sight.is_some())
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(fog) = settings.get::<ComponentFogOfWar>() else {
            return;
        };
        let Some(sight) = &fog.sight else {
            return;
        };
        pack_sight(sight, &mut self.viewers, &mut self.reach);
        let device = context.render_context.device();
        let queue = context.render_context.queue();
        // Storage bindings need a buffer of at least one element.
        self.viewer_buffer
            .resize(device, size_of::<GpuViewer>() as u64);
        self.viewer_buffer
            .write_buffer(device, queue, &self.viewers, None);
        self.reach_buffer.resize(device, size_of::<f32>() as u64);
        self.reach_buffer
            .write_buffer(device, queue, &self.reach, None);
        let uniform = FogOfWarUniform {
            color: [
                fog.color.r() as f32 / 255.0,
                fog.color.g() as f32 / 255.0,
                fog.color.b() as f32 / 255.0,
                fog.opacity,
            ],
            camera: context.camera,
            view_to_world: context.view_to_world.into(),
            viewer_count: self.viewers.len() as u32,
            _pad: [0; 3],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
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
        let viewer_buffer = self.viewer_buffer.get_wgpu_buffer();
        let reach_buffer = self.reach_buffer.get_wgpu_buffer();
        let bind_group = self.bind_group.get_or_create(
            (
                layout.clone(),
                io.input.view.clone(),
                depth.view.clone(),
                self.viewer_buffer.generation(),
                self.reach_buffer.generation(),
            ),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fog_of_war_bind_group"),
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
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: viewer_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: reach_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "Fog of war");
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
