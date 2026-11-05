//! The outline pass: draws ink lines of a constant width in pixels where the
//! G-buffer's surfaces end or crease, on display-linear color.

use super::{
    CameraPlanes, NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass,
    PostProcessSettings, PostProcessStage, SceneInput,
};
use crate::component::{ComponentOutline, OutlineStyle, MAX_OUTLINE_STYLES};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use egui_wgpu::wgpu;
use enumflags2::make_bitflags;

/// The widest line the shader draws, in pixels.
pub const MAX_OUTLINE_WIDTH: u32 = 8;

/// One style as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct InkStyle {
    /// Display-linear ink colour in rgb, opacity in a.
    color: [f32; 4],
    width: u32,
    inner_width: u32,
    inner_opacity: f32,
    _padding: f32,
}

/// The outline pass's uniform. Not the editor's selection outline, whose
/// uniform is `OutlineUniform` in `render/outline_renderer.rs`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct InkOutlineUniform {
    camera: CameraPlanes,
    style_count: u32,
    max_width: u32,
    depth_threshold: f32,
    silhouette_threshold: f32,
    normal_cos: f32,
    _padding: [f32; 3],
    styles: [InkStyle; MAX_OUTLINE_STYLES],
}

/// The whole number of pixels a line `width` wide draws: rounded, from 1 to
/// [`MAX_OUTLINE_WIDTH`].
pub fn outline_pixel_width(width: f32) -> u32 {
    let width = if width.is_finite() {
        width.round()
    } else {
        1.0
    };
    (width.max(1.0) as u32).min(MAX_OUTLINE_WIDTH)
}

/// The styles of `outline` the pass draws: the first
/// [`MAX_OUTLINE_STYLES`], with a warning, once, when it has more.
pub fn drawn_outline_styles(outline: &ComponentOutline) -> &[OutlineStyle] {
    if outline.styles.len() > MAX_OUTLINE_STYLES {
        crate::logging::warn_once("outline-styles-over-max".to_string(), || {
            format!(
                "The Outline effect has {} styles; only the first {MAX_OUTLINE_STYLES} are drawn",
                outline.styles.len()
            )
        });
        &outline.styles[..MAX_OUTLINE_STYLES]
    } else {
        &outline.styles
    }
}

/// Ink outlines in the `Ldr` stage, after colour grading and before FXAA.
///
/// It finds edges in the G-buffer around and inside the surfaces whose
/// material picks one of the effect's styles, sorts them into silhouettes
/// and inner lines, and draws every pixel within an edge's reach in its
/// style's ink. It reads the G-buffer's depth, normals and material (whose
/// alpha holds the style), so forward draws, the skybox and particles draw
/// no lines, and the scene depth, so one of them in front of a line hides
/// it. It runs while the settings hold a [`ComponentOutline`].
pub struct OutlinePass {
    shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    bind_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::TextureView,
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
                    size: size_of::<InkOutlineUniform>() as u64,
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

    /// The G-buffer's depth, normals and material to find edges and their
    /// styles in, and the scene depth to hide a line behind a forward draw
    /// or a particle.
    fn inputs(&self) -> PassInputs {
        make_bitflags!(SceneInput::{Depth | GBufferDepth | Normals | Material})
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.get::<ComponentOutline>().is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(outline) = settings.get::<ComponentOutline>() else {
            return;
        };
        let drawn = drawn_outline_styles(outline);
        let mut uniform = InkOutlineUniform {
            camera: context.camera,
            style_count: drawn.len() as u32,
            max_width: 1,
            depth_threshold: outline.depth_threshold.max(0.0),
            silhouette_threshold: outline.silhouette_threshold.max(0.0),
            normal_cos: outline
                .normal_threshold
                .clamp(0.0, 180.0)
                .to_radians()
                .cos(),
            ..Default::default()
        };
        for (slot, style) in uniform.styles.iter_mut().zip(drawn) {
            let color = egui::Rgba::from(style.color);
            *slot = InkStyle {
                color: [
                    color.r(),
                    color.g(),
                    color.b(),
                    style.opacity.clamp(0.0, 1.0),
                ],
                width: outline_pixel_width(style.width),
                inner_width: outline_pixel_width(style.inner_width),
                inner_opacity: style.inner_opacity.clamp(0.0, 1.0),
                _padding: 0.0,
            };
            uniform.max_width = uniform.max_width.max(slot.width).max(slot.inner_width);
        }
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
        let (Some(gbuffer_depth), Some(normals), Some(material)) =
            (io.gbuffer_depth, io.normals, io.material)
        else {
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
                material.view.clone(),
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
                            resource: wgpu::BindingResource::TextureView(&material.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
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
