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
mod tests {
    use super::*;
    use crate::component::ComponentGlobalPostProcessVolume;
    use crate::render::postprocess::{ChainPlan, PassTarget, PlannedPass, PostProcessSettings};
    use crate::render::scene_renderer::half_to_f32;
    use crate::render::{Camera, SceneRenderer, SceneRendererOptions};
    use crate::test_support::{SceneBuilder, TestAssets};
    use nalgebra_glm::{vec3, Vec3};

    const SIZE: (u32, u32) = (32, 32);
    /// Half of [`SIZE`], the size of the occlusion SSAO writes.
    const HALF: (u32, u32) = (16, 16);
    const BYTES_PER_ROW: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    /// Depth the skybox writes.
    const SKYBOX_DEPTH: f32 = 0.9999999;

    fn camera() -> Camera {
        Camera::new(1.0, 60f32.to_radians(), 0.1, 100.0)
    }

    /// The planes the renderer hands SSAO for [`camera`].
    fn planes() -> CameraPlanes {
        let camera = camera();
        CameraPlanes {
            near: camera.near_plane,
            far: camera.far_plane,
            tan_half_fov: CameraPlanes::tan_half_fov_of(&camera.projection),
        }
    }

    /// SSAO on with `radius`. The test images are small, so the corner
    /// tests use a radius several pixels wide.
    fn ssao_with_radius(radius: f32) -> ComponentAmbientOcclusion {
        ComponentAmbientOcclusion {
            radius,
            ..Default::default()
        }
    }

    /// Radius of the corner tests: about ten pixels at the crease.
    const CORNER_RADIUS: f32 = 1.5;

    fn texture(
        render_context: &Arc<RenderContext>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Texture {
        Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("ssao_test_texture"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
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

    const DEPTH_COPY_SHADER: &str = "
@group(0) @binding(0) var values: texture_2d<f32>;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) position: vec4f) -> @builtin(frag_depth) f32 {
    return textureLoad(values, vec2i(position.xy), 0).r;
}
";

    /// The view-space slopes `(x / z, y / z)` of the ray through the center
    /// of pixel `(x, y)` of [`SIZE`], through the render projection itself,
    /// independent of the planes the pass is given.
    fn slopes(x: u32, y: u32) -> (f32, f32) {
        let ndc_x = (x as f32 + 0.5) / SIZE.0 as f32 * 2.0 - 1.0;
        let ndc_y = 1.0 - (y as f32 + 0.5) / SIZE.1 as f32 * 2.0;
        let projection = camera().projection;
        (ndc_x / projection[(0, 0)], ndc_y / projection[(1, 1)])
    }

    /// Uploads one value of `texel_bytes` bytes per pixel of [`SIZE`] into a
    /// new texture of `format`.
    fn upload(
        render_context: &Arc<RenderContext>,
        format: wgpu::TextureFormat,
        texel_bytes: impl Fn(u32, u32) -> Vec<u8>,
    ) -> Texture {
        let target = texture(render_context, SIZE, format);
        let mut data = vec![0u8; (BYTES_PER_ROW * SIZE.1) as usize];
        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                let bytes = texel_bytes(x, y);
                let start = (y * BYTES_PER_ROW) as usize + x as usize * bytes.len();
                data[start..start + bytes.len()].copy_from_slice(&bytes);
            }
        }
        render_context.queue().write_texture(
            target.texture.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BYTES_PER_ROW),
                rows_per_image: Some(SIZE.1),
            },
            target.descriptor.size,
        );
        target
    }

    /// Scene depth of [`SIZE`] seen through [`camera`]'s projection. `view_z` gives the
    /// view depth of the surface along the ray through each pixel center,
    /// passed as its slopes `(x / z, y / z)`, or `None` for sky, which gets
    /// the raw depth `sky`.
    fn scene_depth(
        render_context: &Arc<RenderContext>,
        view_z: impl Fn(f32, f32) -> Option<f32>,
        sky: f32,
    ) -> Texture {
        // Depth32Float cannot be a copy destination, so the values go
        // through an R32Float texture and a pass that writes frag_depth.
        let values = upload(render_context, wgpu::TextureFormat::R32Float, |x, y| {
            let (sx, sy) = slopes(x, y);
            let projection = camera().projection;
            let value = view_z(sx, sy)
                .map(|z| {
                    let clip = projection * nalgebra_glm::vec4(0.0, 0.0, z, 1.0);
                    clip.z / clip.w
                })
                .unwrap_or(sky);
            value.to_le_bytes().to_vec()
        });

        let device = render_context.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao_test_depth"),
            source: wgpu::ShaderSource::Wgsl(DEPTH_COPY_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao_test_depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao_test_depth"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ssao_test_depth"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_compare: wgpu::CompareFunction::Always,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssao_test_depth"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&values.view),
            }],
        });
        let target = texture(render_context, SIZE, wgpu::TextureFormat::Depth32Float);
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ssao_test_depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                &target.view,
                1.0,
                None,
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        render_context.queue().submit(Some(encoder.finish()));
        target
    }

    fn f32_to_half(value: f32) -> u16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let mantissa = bits & 0x7f_ffff;
        if value == 0.0 {
            sign
        } else if exponent >= 0x1f {
            sign | 0x7c00
        } else if exponent <= 0 {
            sign
        } else {
            sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
        }
    }

    /// The octahedral encoding of the unit vector `n`, as
    /// `docs/design/rendering/gbuffer.md` defines it.
    fn octahedral_encode(n: Vec3) -> [f32; 2] {
        let projected = n.xy() / (n.x.abs() + n.y.abs() + n.z.abs());
        if n.z >= 0.0 {
            return [projected.x, projected.y];
        }
        let sign = |v: f32| if v >= 0.0 { 1.0 } else { -1.0 };
        [
            (1.0 - projected.y.abs()) * sign(projected.x),
            (1.0 - projected.x.abs()) * sign(projected.y),
        ]
    }

    /// A G-buffer `normals` target of [`SIZE`]: `normal` gives the view-space
    /// geometric normal at each pixel's slopes, stored in BA. RG holds the
    /// shading normal, which SSAO must not read: it is set facing away from
    /// the camera.
    fn gbuffer_normals(
        render_context: &Arc<RenderContext>,
        normal: impl Fn(f32, f32) -> Vec3,
    ) -> Texture {
        upload(render_context, wgpu::TextureFormat::Rgba16Float, |x, y| {
            let (sx, sy) = slopes(x, y);
            let shading = octahedral_encode(vec3(0.0, 0.0, 1.0));
            let geometric = octahedral_encode(normal(sx, sy).normalize());
            [shading[0], shading[1], geometric[0], geometric[1]]
                .iter()
                .flat_map(|value| f32_to_half(*value).to_le_bytes())
                .collect()
        })
    }

    /// Every texel of an `Rg16Float` texture of [`HALF`], row by row.
    fn read_half(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 2]> {
        let size = texture.descriptor.size;
        assert_eq!((size.width, size.height), HALF);
        let device = render_context.device();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao_test_readback"),
            size: (BYTES_PER_ROW * size.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(BYTES_PER_ROW),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        render_context.queue().submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
        let _ = device.poll(wgpu::Maintain::Wait);
        let bytes = slice.get_mapped_range();
        let mut texels = Vec::new();
        for y in 0..size.height {
            for x in 0..size.width {
                let start = (y * BYTES_PER_ROW + x * 4) as usize;
                texels.push(std::array::from_fn(|channel| {
                    let at = start + channel * 2;
                    half_to_f32(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
                }));
            }
        }
        drop(bytes);
        buffer.unmap();
        texels
    }

    /// Runs SSAO with `settings` over `depth` and `normals`, and returns the
    /// visibility of each texel of the blurred occlusion it hands the
    /// lighting pass: 1 is unoccluded.
    fn run_ssao(
        context: &ReadOnlyAssetContext,
        settings: &ComponentAmbientOcclusion,
        depth: &Texture,
        normals: &Texture,
    ) -> Vec<f32> {
        let render_context = context.render_context.clone();
        let mut pass = SsaoPass::new(context).expect("ssao loads");
        let mut encoder = render_context
            .device()
            .create_command_encoder(&Default::default());
        let occlusion = pass
            .render(
                &render_context,
                &mut encoder,
                Some(settings),
                SsaoInputs {
                    depth,
                    normals,
                    camera: planes(),
                },
            )
            .expect("ssao runs");
        render_context.queue().submit(Some(encoder.finish()));
        read_half(&render_context, occlusion)
            .iter()
            .map(|texel| texel[0])
            .collect()
    }

    /// The camera-facing normal of a flat plane.
    fn facing(_: f32, _: f32) -> Vec3 {
        vec3(0.0, 0.0, -1.0)
    }

    /// The view depth of two walls at right angles, meeting in a vertical
    /// crease 4 units in front of the camera, down the middle of the image:
    /// z + |x| = 4.
    fn corner(x: f32) -> f32 {
        4.0 / (1.0 + x.abs())
    }

    /// The normals of [`corner`]'s walls, facing the camera and each other.
    fn corner_normal(x: f32, _: f32) -> Vec3 {
        if x < 0.0 {
            vec3(1.0, 0.0, -1.0)
        } else {
            vec3(-1.0, 0.0, -1.0)
        }
    }

    #[test]
    fn a_flat_plane_facing_the_camera_is_not_occluded() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let depth = scene_depth(&context.render_context, |_, _| Some(4.0), 1.0);
        let normals = gbuffer_normals(&context.render_context, facing);

        for radius in [ComponentAmbientOcclusion::default().radius, CORNER_RADIUS] {
            let visibility = run_ssao(&context, &ssao_with_radius(radius), &depth, &normals);

            for (index, value) in visibility.iter().enumerate() {
                assert!(
                    *value >= 0.999,
                    "texel {index} is occluded at radius {radius}: {value}"
                );
            }
        }
    }

    #[test]
    fn the_hemisphere_follows_the_gbuffer_normal() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // The flat plane facing the camera, whose G-buffer normal says it is
        // turned 75 degrees: the hemisphere around that normal reaches
        // behind the plane, so SSAO occludes what a normal rebuilt from the
        // depth would leave open.
        let depth = scene_depth(&context.render_context, |_, _| Some(4.0), 1.0);
        let angle = 75f32.to_radians();
        let normals = gbuffer_normals(&context.render_context, |_, _| {
            vec3(angle.sin(), 0.0, -angle.cos())
        });

        let visibility = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth, &normals);

        let at = |x: u32, y: u32| visibility[(y * HALF.0 + x) as usize];
        for y in 4..12 {
            for x in 4..12 {
                assert!(
                    at(x, y) < 0.9,
                    "the tilted normal at ({x}, {y}) reaches behind the plane: visibility {}",
                    at(x, y)
                );
            }
        }
    }

    #[test]
    fn an_inside_corner_is_occluded() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let depth = scene_depth(&context.render_context, |x, _| Some(corner(x)), 1.0);
        let normals = gbuffer_normals(&context.render_context, corner_normal);

        let visibility = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth, &normals);

        let at = |x: u32, y: u32| visibility[(y * HALF.0 + x) as usize];
        for y in 4..12 {
            for x in [7, 8] {
                assert!(
                    at(x, y) < 0.9,
                    "the crease at ({x}, {y}) is occluded: visibility {}",
                    at(x, y)
                );
            }
            for x in [0, 15] {
                assert!(
                    at(x, y) > 0.99,
                    "the flat wall at ({x}, {y}) is not: visibility {}",
                    at(x, y)
                );
            }
            assert!(
                at(7, y) < at(5, y) && at(5, y) <= at(2, y) + 1e-3,
                "occlusion fades away from the crease at row {y}: {} {} {}",
                at(7, y),
                at(5, y),
                at(2, y)
            );
        }
    }

    #[test]
    fn the_sky_is_unoccluded() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let normals = gbuffer_normals(&context.render_context, |x, y| corner_normal(x + 0.3, y));
        for sky in [1.0, SKYBOX_DEPTH] {
            // A corner on the left half and sky on the right, so the sky
            // borders occluded geometry.
            let depth = scene_depth(
                &context.render_context,
                |x, _| (x < 0.0).then(|| corner(x + 0.3)),
                sky,
            );

            let visibility = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth, &normals);

            for y in 0..HALF.1 {
                for x in HALF.0 / 2..HALF.0 {
                    assert_eq!(
                        visibility[(y * HALF.0 + x) as usize],
                        1.0,
                        "sky at ({x}, {y}), depth {sky}, is unoccluded"
                    );
                }
            }
            assert!(
                visibility.iter().any(|value| *value < 0.9),
                "the geometry beside the sky is occluded"
            );
        }
    }

    #[test]
    fn the_targets_are_half_the_gbuffer_size_and_freed_when_off() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let mut pass = SsaoPass::new(&context).expect("ssao loads");
        assert_eq!(pass.target_size(), None, "nothing is allocated up front");

        let render =
            |pass: &mut SsaoPass, settings: Option<ComponentAmbientOcclusion>, size: (u32, u32)| {
                let depth = texture(&render_context, size, wgpu::TextureFormat::Depth32Float);
                let normals = texture(&render_context, size, wgpu::TextureFormat::Rgba16Float);
                let mut encoder = render_context
                    .device()
                    .create_command_encoder(&Default::default());
                let ran = pass
                    .render(
                        &render_context,
                        &mut encoder,
                        settings.as_ref(),
                        SsaoInputs {
                            depth: &depth,
                            normals: &normals,
                            camera: planes(),
                        },
                    )
                    .is_some();
                render_context.queue().submit(Some(encoder.finish()));
                (ran, pass.target_size())
            };

        let on = Some(ComponentAmbientOcclusion::default());
        assert_eq!(render(&mut pass, on, (32, 16)), (true, Some((16, 8))));
        assert_eq!(
            render(&mut pass, on, (5, 3)),
            (true, Some((3, 2))),
            "odd sizes round up and a new size refits the targets"
        );
        assert_eq!(
            render(&mut pass, None, (5, 3)),
            (false, None),
            "a frame without SSAO frees the targets"
        );
        let zero = Some(ComponentAmbientOcclusion {
            intensity: 0.0,
            ..Default::default()
        });
        assert_eq!(
            render(&mut pass, zero, (5, 3)),
            (false, None),
            "an intensity of zero occludes nothing, so it runs nothing"
        );
        render(&mut pass, on, (5, 3));
        pass.release_targets();
        assert_eq!(pass.target_size(), None, "releasing frees the targets");
    }

    #[test]
    fn the_volume_switches_ssao_and_the_chain_plan_never_holds_it() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let volume = builder.spawn("volume");
        builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
        let mut scene = builder.finish();
        let mut renderer = SceneRenderer::new(
            &context,
            SceneRendererOptions {
                samples: 1,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        let mut render = |scene: &crate::scene::Scene| {
            renderer.render_scene_base(&camera(), &Default::default(), scene, None, None);
            renderer.finalize_scene();
            let names: Vec<_> = renderer
                .post_process()
                .passes()
                .iter()
                .map(|pass| pass.name())
                .collect();
            assert!(!names.contains(&"ssao"), "SSAO is not a chain pass");
            let tonemap = names
                .iter()
                .position(|name| *name == "tonemap")
                .expect("the renderer tone maps");
            assert_eq!(
                *renderer.last_post_process_plan(),
                ChainPlan {
                    passes: vec![PlannedPass {
                        pass: tonemap,
                        input: PassTarget::Source,
                        output: PassTarget::Output,
                    }],
                    skipped: vec![],
                    intermediates: 0,
                    depth: false,
                    particle_depth: false,
                    normals: false,
                    albedo: false,
                    gbuffer_depth: false,
                    material: false,
                },
                "with or without SSAO the chain tone maps alone and builds no depth"
            );
            renderer.ambient_occlusion().target_size()
        };

        assert_eq!(render(&scene), None, "SSAO is off without the component");
        scene.add_component(volume, ComponentAmbientOcclusion::default());
        assert_eq!(
            render(&scene),
            Some((SIZE.0 / 2, SIZE.1 / 2)),
            "adding the component runs SSAO"
        );
        assert!(scene.set_component_enabled::<ComponentAmbientOcclusion>(volume, false));
        assert_eq!(render(&scene), None, "disabling the component stops it");
        assert!(scene.set_component_enabled::<ComponentAmbientOcclusion>(volume, true));
        assert!(render(&scene).is_some(), "enabling it runs SSAO again");
        scene.remove_component::<ComponentAmbientOcclusion>(volume);
        assert_eq!(render(&scene), None, "removing the component stops it");
    }

    #[test]
    fn ssao_is_off_by_default_and_a_partial_component_loads_with_defaults() {
        assert_eq!(
            PostProcessSettings::default()
                .get::<ComponentAmbientOcclusion>()
                .copied(),
            None
        );
        let partial = r#"{ "radius": 2.0 }"#;
        let ao: ComponentAmbientOcclusion = serde_json::from_str(partial).expect("ssao loads");
        assert_eq!(
            ao,
            ComponentAmbientOcclusion {
                radius: 2.0,
                ..Default::default()
            }
        );
    }
}
