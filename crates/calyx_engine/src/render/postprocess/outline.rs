//! The outline pass: draws ink lines of a constant width in pixels where the
//! G-buffer's surfaces end or crease, on display-linear color.

use super::{
    NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass, PostProcessSettings,
    PostProcessStage, SceneInput,
};
use crate::component::ComponentOutline;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;
use enumflags2::make_bitflags;

/// The widest line the shader draws, in pixels: twice its `MAX_REACH`.
pub const MAX_OUTLINE_WIDTH: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct OutlineUniform {
    /// Display-linear ink colour in rgb, opacity in a.
    color: [f32; 4],
    near_plane: f32,
    far_plane: f32,
    width: u32,
    depth_threshold: f32,
    normal_cos: f32,
    _padding: f32,
    tan_half_fov: [f32; 2],
}

/// The whole number of pixels a line of `outline.width` draws: rounded, from
/// 1 to [`MAX_OUTLINE_WIDTH`].
pub fn outline_pixel_width(outline: &ComponentOutline) -> u32 {
    let width = if outline.width.is_finite() {
        outline.width.round()
    } else {
        1.0
    };
    (width.max(1.0) as u32).min(MAX_OUTLINE_WIDTH)
}

/// Ink outlines in the `Ldr` stage, after colour grading and before FXAA.
///
/// It finds edges in the G-buffer, between an opaque PBR surface and the
/// background, across a jump in distance the surfaces on either side do not
/// predict, or across a sharp turn of their geometric normals, and draws
/// every pixel within the line's reach of one in the ink colour. It reads
/// the G-buffer's depth and normals, so forward draws, the skybox and
/// particles draw no lines, and the scene depth, so one of them in front of
/// a line hides it. It runs while the settings hold a
/// [`ComponentOutline`].
pub struct OutlinePass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
    )>,
}

impl OutlinePass {
    /// Shader asset the pass draws with.
    pub const SHADER: &'static str = "shaders/postprocess/outline";

    /// Creates the pass, or reports why its shader did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let shader = SceneRendererError::load::<Shader>(context, Self::SHADER)?;
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("outline_uniform_buffer"),
                    size: size_of::<OutlineUniform>() as u64,
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

impl PostProcessPass for OutlinePass {
    fn name(&self) -> &str {
        "outline"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Ldr
    }

    /// The G-buffer's depth and normals to find edges in, and the scene
    /// depth to hide a line behind a forward draw or a particle.
    fn inputs(&self) -> PassInputs {
        make_bitflags!(SceneInput::{Depth | GBufferDepth | Normals})
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.outline.is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(outline) = &settings.outline else {
            return;
        };
        let color = egui::Rgba::from(outline.color);
        let uniform = OutlineUniform {
            color: [
                color.r(),
                color.g(),
                color.b(),
                outline.opacity.clamp(0.0, 1.0),
            ],
            near_plane: context.camera.near,
            far_plane: context.camera.far,
            width: outline_pixel_width(outline),
            depth_threshold: outline.depth_threshold.max(0.0),
            normal_cos: outline
                .normal_threshold
                .clamp(0.0, 180.0)
                .to_radians()
                .cos(),
            _padding: 0.0,
            tan_half_fov: context.camera.tan_half_fov,
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
        let (Some(gbuffer_depth), Some(normals)) = (io.gbuffer_depth, io.normals) else {
            // Before the renderer has drawn a G-buffer there is nothing to
            // outline.
            encoder.copy_texture_to_texture(
                io.input.texture.as_image_copy(),
                io.output.texture.as_image_copy(),
                io.output.descriptor.size,
            );
            return Ok(());
        };
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
            (
                layout.clone(),
                io.input.view.clone(),
                gbuffer_depth.view.clone(),
                normals.view.clone(),
                depth.view.clone(),
            ),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("outline_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&gbuffer_depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&normals.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, "Outline");
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
