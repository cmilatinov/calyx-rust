//! Screen-space ambient occlusion: measures, from the G-buffer's depth and
//! normals, how much ambient light reaches each opaque PBR surface. The
//! deferred lighting pass multiplies it into the ambient and image-based
//! light only. See the Ambient Occlusion section of
//! `docs/design/post-processing/architecture.md`.

use super::{CameraPlanes, NoPipeline};
use crate::assets::texture::Texture;
use crate::component::ComponentAmbientOcclusion;
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::render::{
    CachedBindGroup, PipelineOptions, RenderContext, RenderUtils, SceneRendererError, Shader,
};
use egui_wgpu::wgpu;
use std::sync::Arc;

const MAX_SAMPLES: usize = ComponentAmbientOcclusion::MAX_SAMPLES as usize;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SsaoUniform {
    camera: CameraPlanes,
    radius: f32,
    bias: f32,
    intensity: f32,
    sample_count: u32,
    kernel: [[f32; 4]; MAX_SAMPLES],
}

/// `count` sample offsets in the unit hemisphere around +z, the same for a
/// given `count` every frame. Offsets cluster toward the center, so nearby
/// geometry weighs more, and keep some height above the tangent plane, so a
/// flat surface does not occlude itself.
fn hemisphere_kernel(count: usize) -> [[f32; 4]; MAX_SAMPLES] {
    // xorshift32 with a fixed seed: deterministic, and good enough to spread
    // a few dozen directions.
    let mut state = 0x9e37_79b9_u32;
    let mut random = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    };
    let mut kernel = [[0.0; 4]; MAX_SAMPLES];
    for (index, offset) in kernel.iter_mut().take(count).enumerate() {
        let direction = nalgebra_glm::normalize(&nalgebra_glm::vec3(
            random() * 2.0 - 1.0,
            random() * 2.0 - 1.0,
            0.15 + random() * 0.85,
        ));
        let progress = index as f32 / count as f32;
        let scale = 0.1 + 0.9 * progress * progress;
        let direction = direction * random().max(0.1) * scale;
        *offset = [direction.x, direction.y, direction.z, 0.0];
    }
    kernel
}

/// The occlusion texture and its blurred copy, at half the G-buffer's size.
struct Targets {
    occlusion: Texture,
    blurred: Texture,
}

/// What [`SsaoPass::render`] reads: the frame's G-buffer and the camera
/// that drew it.
pub struct SsaoInputs<'a> {
    /// The G-buffer's single-sample depth. SSAO runs before anything but the
    /// geometry pass draws into it, so it holds the opaque PBR surfaces only.
    pub depth: &'a Texture,
    /// The G-buffer's `normals` target: the view-space geometric normal,
    /// octahedral, in BA.
    pub normals: &'a Texture,
    /// The planes and field of view of the camera that drew the G-buffer.
    pub camera: CameraPlanes,
}

/// Screen-space ambient occlusion, between the geometry pass and the
/// deferred lighting pass.
///
/// At half resolution it takes each texel's view-space position from the
/// G-buffer's depth and its geometric normal from the G-buffer, then counts
/// how many samples of a hemisphere around the normal, rotated per texel by
/// a 4x4 noise pattern, lie behind the depth. A depth-aware 4x4 blur removes
/// the noise. The lighting pass upsamples the result, again weighted by
/// depth, and multiplies it into the ambient and image-based light.
///
/// The pass owns its two half-resolution targets. It creates them on the
/// first frame SSAO runs and frees them on resize and on any frame it does
/// not run.
pub struct SsaoPass {
    occlusion_shader: Ref<Shader>,
    blur_shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    targets: Option<Targets>,
    occlusion_bind_group:
        CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::TextureView)>,
    blur_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
    /// Whether a shader without a pipeline has been reported, so a broken
    /// shader warns once instead of every frame.
    warned: bool,
}

impl SsaoPass {
    /// Shader that computes the occlusion.
    pub const OCCLUSION_SHADER: &'static str = "shaders/postprocess/ssao";
    /// Shader of the depth-aware blur.
    pub const BLUR_SHADER: &'static str = "shaders/postprocess/ssao_blur";
    /// Format of the occlusion targets: visibility in red, view depth in
    /// green for the depth-aware blur and upsample.
    pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

    /// Creates the pass, or reports why a shader did not load. It allocates
    /// no texture until SSAO first runs.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let uniform_buffer =
            context
                .render_context
                .device()
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ssao_uniform_buffer"),
                    size: size_of::<SsaoUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        Ok(Self {
            occlusion_shader: SceneRendererError::load::<Shader>(context, Self::OCCLUSION_SHADER)?,
            blur_shader: SceneRendererError::load::<Shader>(context, Self::BLUR_SHADER)?,
            uniform_buffer,
            targets: None,
            occlusion_bind_group: Default::default(),
            blur_bind_group: Default::default(),
            warned: false,
        })
    }

    /// The size of the occlusion targets the pass holds, if any.
    pub fn target_size(&self) -> Option<(u32, u32)> {
        self.targets.as_ref().map(|targets| {
            let size = targets.occlusion.descriptor.size;
            (size.width, size.height)
        })
    }

    /// Records the occlusion of `inputs` with `settings`, and returns the
    /// blurred half-resolution occlusion for the lighting pass: visibility
    /// in red, view depth in green.
    ///
    /// Returns `None`, and frees the targets, when `settings` is `None` or
    /// its intensity is zero, which leaves every surface unoccluded, or when
    /// a shader has no pipeline, which it reports once.
    pub fn render(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        settings: Option<&ComponentAmbientOcclusion>,
        inputs: SsaoInputs,
    ) -> Option<&Texture> {
        let Some(settings) = settings.filter(|ao| ao.intensity > 0.0) else {
            self.release_targets();
            return None;
        };
        match self.record(render_context, encoder, settings, &inputs) {
            Ok(()) => {
                self.warned = false;
                self.targets.as_ref().map(|targets| &targets.blurred)
            }
            Err(error) => {
                if !self.warned {
                    log::warn!(
                        "SSAO is off: shader `{}` has no pipeline, so the ambient light is \
                         left unoccluded",
                        error.shader
                    );
                    self.warned = true;
                }
                self.release_targets();
                None
            }
        }
    }

    /// Frees the targets and the bind groups that hold them.
    pub fn release_targets(&mut self) {
        self.targets = None;
        self.occlusion_bind_group.clear();
        self.blur_bind_group.clear();
    }

    fn record(
        &mut self,
        render_context: &Arc<RenderContext>,
        encoder: &mut wgpu::CommandEncoder,
        ao: &ComponentAmbientOcclusion,
        inputs: &SsaoInputs,
    ) -> Result<(), NoPipeline> {
        let samples = ao.samples();
        let uniform = SsaoUniform {
            camera: inputs.camera,
            radius: ao.radius.max(0.0),
            bias: ao.bias.max(0.0),
            intensity: ao.intensity.max(0.0),
            sample_count: samples,
            kernel: hemisphere_kernel(samples as usize),
        };
        render_context
            .queue()
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));
        let size = inputs.depth.descriptor.size;
        self.fit_targets(render_context, (size.width, size.height));
        let device = render_context.device();
        let Self {
            occlusion_shader,
            blur_shader,
            uniform_buffer,
            targets,
            occlusion_bind_group,
            blur_bind_group,
            ..
        } = self;
        let targets = targets.as_ref().expect("the targets were just fitted");

        // Occlusion from the G-buffer's depth and normals, at half
        // resolution.
        let (pipeline, layout) = Self::pipeline(occlusion_shader)?;
        let (depth, normals) = (&inputs.depth.view, &inputs.normals.view);
        let bind_group = occlusion_bind_group.get_or_create(
            (layout.clone(), depth.clone(), normals.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ssao_occlusion_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(depth),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(normals),
                        },
                    ],
                })
            },
        );
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &targets.occlusion.view, "SSAO Occlusion");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        drop(render_pass);

        // Depth-aware blur that removes the rotation noise.
        let (pipeline, layout) = Self::pipeline(blur_shader)?;
        let occlusion = &targets.occlusion.view;
        let bind_group = blur_bind_group.get_or_create((layout.clone(), occlusion.clone()), || {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao_blur_bind_group"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(occlusion),
                }],
            })
        });
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &targets.blurred.view, "SSAO Blur");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }

    /// Half of `size`, rounded up, so every full-resolution pixel has an
    /// occlusion texel.
    fn half_size(size: (u32, u32)) -> (u32, u32) {
        (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1))
    }

    /// Makes sure both targets exist at half of `size`.
    fn fit_targets(&mut self, render_context: &Arc<RenderContext>, size: (u32, u32)) {
        let half = Self::half_size(size);
        if self.target_size() == Some(half) {
            return;
        }
        self.release_targets();
        self.targets = Some(Targets {
            occlusion: Self::create_target(render_context.clone(), half, "ssao_occlusion"),
            blurred: Self::create_target(render_context.clone(), half, "ssao_blurred"),
        });
    }

    /// An occlusion target of `size`, which the lighting pass binds and
    /// tests copy out.
    pub(crate) fn create_target(
        render_context: Arc<RenderContext>,
        size: (u32, u32),
        label: &'static str,
    ) -> Texture {
        Texture::new(
            render_context,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Self::TARGET_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    /// The pipeline of `shader` drawing into an occlusion target, and its
    /// bind group layout.
    fn pipeline(
        shader: &Ref<Shader>,
    ) -> Result<(wgpu::RenderPipeline, wgpu::BindGroupLayout), NoPipeline> {
        let shader = shader.read();
        let pipeline = shader
            .pipeline(
                &PipelineOptions::fullscreen_builder()
                    .target(RenderUtils::color_default(Self::TARGET_FORMAT))
                    .build(),
            )
            .ok_or_else(|| NoPipeline::of(&shader))?;
        Ok((pipeline, shader.bind_group_layouts[0].clone()))
    }
}

#[cfg(test)]
mod tests;
