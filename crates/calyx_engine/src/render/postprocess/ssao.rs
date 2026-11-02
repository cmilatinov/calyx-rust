//! The screen-space ambient occlusion pass: darkens creases and contact
//! areas from the scene's depth.

use super::{
    NoPipeline, PassContext, PassInputs, PassIo, PostProcessPass, PostProcessSettings,
    PostProcessStage, SceneInput,
};
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
    near: f32,
    far: f32,
    tan_half_fov: [f32; 2],
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

/// The occlusion texture and its blurred copy, at half the chain's size.
struct Targets {
    occlusion: Texture,
    blurred: Texture,
}

/// Screen-space ambient occlusion in the `Hdr` stage, before any other
/// effect there.
///
/// At half resolution it rebuilds each pixel's view-space position and
/// normal from depth, then counts how many samples of a hemisphere around
/// the normal, rotated per pixel by a 4x4 noise pattern, lie behind the
/// depth buffer. A depth-aware 4x4 blur removes the noise, and the
/// composite upsamples the result, again weighted by depth, and multiplies
/// it into the HDR color. Sky pixels and alpha pass through.
///
/// The pass owns its two half-resolution targets. It creates them on the
/// first frame SSAO runs and frees them on resize and on any frame it does
/// not run.
pub struct SsaoPass {
    occlusion_shader: Ref<Shader>,
    blur_shader: Ref<Shader>,
    composite_shader: Ref<Shader>,
    uniform_buffer: wgpu::Buffer,
    targets: Option<Targets>,
    occlusion_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
    blur_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
    composite_bind_group: CachedBindGroup<(
        wgpu::BindGroupLayout,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
    )>,
}

impl SsaoPass {
    /// Shader that computes the occlusion.
    pub const OCCLUSION_SHADER: &'static str = "shaders/postprocess/ssao";
    /// Shader of the depth-aware blur.
    pub const BLUR_SHADER: &'static str = "shaders/postprocess/ssao_blur";
    /// Shader that multiplies the occlusion into the scene.
    pub const COMPOSITE_SHADER: &'static str = "shaders/postprocess/ssao_composite";
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
            composite_shader: SceneRendererError::load::<Shader>(context, Self::COMPOSITE_SHADER)?,
            uniform_buffer,
            targets: None,
            occlusion_bind_group: Default::default(),
            blur_bind_group: Default::default(),
            composite_bind_group: Default::default(),
        })
    }

    /// The size of the occlusion targets the pass holds, if any.
    pub fn target_size(&self) -> Option<(u32, u32)> {
        self.targets.as_ref().map(|targets| {
            let size = targets.occlusion.descriptor.size;
            (size.width, size.height)
        })
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
        self.targets = Some(Targets {
            occlusion: Self::create_target(render_context.clone(), half, "ssao_occlusion"),
            blurred: Self::create_target(render_context.clone(), half, "ssao_blurred"),
        });
    }

    fn create_target(
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
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    /// The pipeline of `shader` drawing into `format`, and its bind group
    /// layout.
    fn pipeline(
        shader: &Ref<Shader>,
        format: wgpu::TextureFormat,
    ) -> Result<(wgpu::RenderPipeline, wgpu::BindGroupLayout), NoPipeline> {
        let shader = shader.read();
        let pipeline = shader
            .pipeline(
                &PipelineOptions::fullscreen_builder()
                    .target(RenderUtils::color_default(format))
                    .build(),
            )
            .ok_or_else(|| NoPipeline::of(&shader))?;
        Ok((pipeline, shader.bind_group_layouts[0].clone()))
    }
}

impl PostProcessPass for SsaoPass {
    fn name(&self) -> &str {
        "ssao"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Hdr
    }

    /// Occlusion comes from the scene depth. The particle depth tells the
    /// composite which pixels a particle in front of the scene covers, which it
    /// leaves undarkened.
    fn inputs(&self) -> PassInputs {
        SceneInput::Depth | SceneInput::ParticleDepth
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.ambient_occlusion.is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(ao) = &settings.ambient_occlusion else {
            return;
        };
        let samples = ao.samples();
        let uniform = SsaoUniform {
            near: context.camera.near,
            far: context.camera.far,
            tan_half_fov: context.camera.tan_half_fov,
            radius: ao.radius.max(0.0),
            bias: ao.bias.max(0.0),
            intensity: ao.intensity.max(0.0),
            sample_count: samples,
            kernel: hemisphere_kernel(samples as usize),
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
            .expect("the chain hands depth to a pass that asks for it");
        let particle_depth = io
            .particle_depth
            .expect("the chain hands particle depth to a pass that asks for it");
        self.fit_targets(context.render_context, context.size);
        let device = context.render_context.device();
        let Self {
            occlusion_shader,
            blur_shader,
            composite_shader,
            uniform_buffer,
            targets,
            occlusion_bind_group,
            blur_bind_group,
            composite_bind_group,
        } = self;
        let targets = targets.as_ref().expect("the targets were just fitted");

        // Occlusion from depth, at half resolution.
        let (pipeline, layout) = Self::pipeline(occlusion_shader, Self::TARGET_FORMAT)?;
        let bind_group =
            occlusion_bind_group.get_or_create((layout.clone(), depth.view.clone()), || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ssao_occlusion_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            });
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &targets.occlusion.view, "SSAO Occlusion");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        drop(render_pass);

        // Depth-aware blur that removes the rotation noise.
        let (pipeline, layout) = Self::pipeline(blur_shader, Self::TARGET_FORMAT)?;
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
        drop(render_pass);

        // Multiply the upsampled occlusion into the scene color.
        let (pipeline, layout) = Self::pipeline(composite_shader, io.output.descriptor.format)?;
        let blurred = &targets.blurred.view;
        let bind_group = composite_bind_group.get_or_create(
            (
                layout.clone(),
                io.input.view.clone(),
                blurred.clone(),
                depth.view.clone(),
                particle_depth.view.clone(),
            ),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ssao_composite_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(blurred),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&depth.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&particle_depth.view),
                        },
                    ],
                })
            },
        );
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &io.output.view, "SSAO Composite");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        Ok(())
    }

    fn release_targets(&mut self) {
        self.targets = None;
        self.occlusion_bind_group.clear();
        self.blur_bind_group.clear();
        self.composite_bind_group.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::ComponentGlobalPostProcessVolume;
    use crate::render::postprocess::{
        CameraPlanes, ChainPlan, ChainTargets, PassTarget, PlannedPass, PostProcessChain,
    };
    use crate::render::scene_renderer::half_to_f32;
    use crate::render::{Camera, SceneRenderer, SceneRendererOptions};
    use crate::test_support::{SceneBuilder, TestAssets};

    const SIZE: (u32, u32) = (32, 32);
    const BYTES_PER_ROW: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    /// Linear HDR gray the scene is cleared to, with partial alpha.
    const COLOR: [f32; 4] = [0.5, 0.5, 0.5, 0.75];
    /// Depth the skybox writes.
    const SKYBOX_DEPTH: f32 = 0.9999999;

    fn camera() -> Camera {
        Camera::new(1.0, 60f32.to_radians(), 0.1, 100.0)
    }

    /// The planes the renderer hands the chain for [`camera`].
    fn planes() -> CameraPlanes {
        let camera = camera();
        CameraPlanes {
            near: camera.near_plane,
            far: camera.far_plane,
            tan_half_fov: CameraPlanes::tan_half_fov_of(&camera.projection),
        }
    }

    fn ssao() -> PostProcessSettings {
        ssao_with_radius(ComponentAmbientOcclusion::default().radius)
    }

    /// SSAO on with `radius`. The test images are small, so the corner
    /// tests use a radius several pixels wide.
    fn ssao_with_radius(radius: f32) -> PostProcessSettings {
        PostProcessSettings {
            ambient_occlusion: Some(ComponentAmbientOcclusion {
                radius,
                ..Default::default()
            }),
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
        let values = texture(render_context, SIZE, wgpu::TextureFormat::R32Float);
        let mut data = vec![0u8; (BYTES_PER_ROW * SIZE.1) as usize];
        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                let ndc_x = (x as f32 + 0.5) / SIZE.0 as f32 * 2.0 - 1.0;
                let ndc_y = 1.0 - (y as f32 + 0.5) / SIZE.1 as f32 * 2.0;
                // Through the render projection itself, independent of the
                // planes the pass is given.
                let projection = camera().projection;
                let slopes = (ndc_x / projection[(0, 0)], ndc_y / projection[(1, 1)]);
                let value = view_z(slopes.0, slopes.1)
                    .map(|z| {
                        let clip = projection * nalgebra_glm::vec4(0.0, 0.0, z, 1.0);
                        clip.z / clip.w
                    })
                    .unwrap_or(sky);
                let start = (y * BYTES_PER_ROW + x * 4) as usize;
                data[start..start + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        render_context.queue().write_texture(
            values.texture.as_image_copy(),
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BYTES_PER_ROW),
                rows_per_image: Some(SIZE.1),
            },
            values.descriptor.size,
        );

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

    /// Every texel of an `Rgba16Float` texture, row by row.
    fn read_all(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 4]> {
        let size = texture.descriptor.size;
        assert!(size.width * 8 <= BYTES_PER_ROW);
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
                let start = (y * BYTES_PER_ROW + x * 8) as usize;
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

    /// Runs a chain of SSAO alone with `settings` over a source cleared to
    /// [`COLOR`] with scene depth `depth`, and returns the output.
    fn run_ssao(
        context: &ReadOnlyAssetContext,
        settings: &PostProcessSettings,
        depth: &Texture,
    ) -> Vec<[f32; 4]> {
        run_ssao_with_particles(context, settings, depth, None)
    }

    /// [`run_ssao`] with `particle_depth` as the scene depth with the
    /// particles drawn in.
    fn run_ssao_with_particles(
        context: &ReadOnlyAssetContext,
        settings: &PostProcessSettings,
        depth: &Texture,
        particle_depth: Option<&Texture>,
    ) -> Vec<[f32; 4]> {
        let render_context = context.render_context.clone();
        let mut chain = PostProcessChain::new(context).expect("chain shaders load");
        chain.push(Box::new(SsaoPass::new(context).expect("ssao loads")));
        chain.set_camera(planes());
        let source = texture(&render_context, SIZE, wgpu::TextureFormat::Rgba16Float);
        let output = texture(&render_context, SIZE, wgpu::TextureFormat::Rgba16Float);
        let mut encoder = render_context
            .device()
            .create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ssao_test_clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &source.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: COLOR[0] as f64,
                        g: COLOR[1] as f64,
                        b: COLOR[2] as f64,
                        a: COLOR[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        }));
        let plan = chain.run(
            &render_context,
            &mut encoder,
            settings,
            ChainTargets {
                source: &source,
                output: &output,
                depth,
                particle_depth,
            },
        );
        assert_eq!(plan.passes.len(), 1, "ssao runs");
        render_context.queue().submit(Some(encoder.finish()));
        read_all(&render_context, &output)
    }

    /// How much of [`COLOR`] is left at each pixel: 1 is unoccluded.
    fn visibility(output: &[[f32; 4]]) -> Vec<f32> {
        output.iter().map(|texel| texel[0] / COLOR[0]).collect()
    }

    #[test]
    fn a_flat_plane_facing_the_camera_is_not_occluded() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let depth = scene_depth(&context.render_context, |_, _| Some(4.0), 1.0);

        for radius in [ComponentAmbientOcclusion::default().radius, CORNER_RADIUS] {
            let output = run_ssao(&context, &ssao_with_radius(radius), &depth);

            for (index, texel) in output.iter().enumerate() {
                for channel in 0..3 {
                    assert!(
                        (texel[channel] - COLOR[channel]).abs() <= 1e-3,
                        "pixel {index} is darkened at radius {radius}: {texel:?}"
                    );
                }
                assert_eq!(texel[3], COLOR[3], "ssao keeps alpha");
            }
        }
    }

    #[test]
    fn an_inside_corner_is_occluded() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // Two walls at right angles, meeting in a vertical crease 4 units in
        // front of the camera, down the middle of the image: z + |x| = 4.
        let depth = scene_depth(
            &context.render_context,
            |x, _| Some(4.0 / (1.0 + x.abs())),
            1.0,
        );

        let output = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth);

        let visibility = visibility(&output);
        let at = |x: u32, y: u32| visibility[(y * SIZE.0 + x) as usize];
        for y in 8..24 {
            for x in [15, 16] {
                assert!(
                    at(x, y) < 0.9,
                    "the crease at ({x}, {y}) is occluded: visibility {}",
                    at(x, y)
                );
            }
            for x in [1, 30] {
                assert!(
                    at(x, y) > 0.99,
                    "the flat wall at ({x}, {y}) is not: visibility {}",
                    at(x, y)
                );
            }
            assert!(
                at(15, y) < at(12, y) && at(12, y) <= at(6, y) + 1e-3,
                "occlusion fades away from the crease at row {y}: {} {} {}",
                at(15, y),
                at(12, y),
                at(6, y)
            );
        }
        for texel in &output {
            assert_eq!(texel[3], COLOR[3], "ssao keeps alpha");
        }
    }

    #[test]
    fn a_thin_surface_between_occlusion_texels_is_not_darkened() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // The inside corner, with a one-pixel post at column 15 standing far in
        // front of its crease. Occlusion texels sit on even pixels, so none
        // of them sees the post: its four nearest texels all lie on the
        // occluded crease behind it, at a depth that rejects every one.
        let p00 = camera().projection[(0, 0)];
        let column = |x: f32| ((x * p00 + 1.0) * 0.5 * SIZE.0 as f32).floor() as u32;
        let depth = scene_depth(
            &context.render_context,
            |x, _| {
                Some(if column(x) == 15 {
                    1.0
                } else {
                    4.0 / (1.0 + x.abs())
                })
            },
            1.0,
        );

        let output = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth);

        let visibility = visibility(&output);
        let at = |x: u32, y: u32| visibility[(y * SIZE.0 + x) as usize];
        for y in 8..24 {
            assert!(
                at(16, y) < 0.9,
                "the crease behind the post at (16, {y}) is occluded: visibility {}",
                at(16, y)
            );
            assert!(
                at(15, y) > 0.999,
                "the post at (15, {y}) takes no occlusion from the crease behind it: visibility {}",
                at(15, y)
            );
        }
    }

    #[test]
    fn a_particle_out_of_depth_is_not_darkened_by_the_geometry_behind_it() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        // The inside corner, with an additive particle 2 units in front of
        // the camera covering the middle of the lower half of the image. It
        // is in the particle depth only, as the particle renderer draws it.
        let corner = |x: f32| 4.0 / (1.0 + x.abs());
        let depth = scene_depth(&context.render_context, |x, _| Some(corner(x)), 1.0);
        let p00 = camera().projection[(0, 0)];
        let p11 = camera().projection[(1, 1)];
        let column = |x: f32| ((x * p00 + 1.0) * 0.5 * SIZE.0 as f32).floor() as u32;
        let row = |y: f32| ((1.0 - y * p11) * 0.5 * SIZE.1 as f32).floor() as u32;
        let covered = |x: u32, y: u32| (12..20).contains(&x) && y >= 16;
        let particle_depth = scene_depth(
            &context.render_context,
            |x, y| {
                Some(if covered(column(x), row(y)) {
                    2.0
                } else {
                    corner(x)
                })
            },
            1.0,
        );

        let output = run_ssao_with_particles(
            &context,
            &ssao_with_radius(CORNER_RADIUS),
            &depth,
            Some(&particle_depth),
        );

        let visibility = visibility(&output);
        let at = |x: u32, y: u32| visibility[(y * SIZE.0 + x) as usize];
        for y in 6..12 {
            assert!(
                at(15, y) < 0.9,
                "the crease above the particle at (15, {y}) is occluded: visibility {}",
                at(15, y)
            );
        }
        for y in 16..SIZE.1 {
            for x in 12..20 {
                assert_eq!(
                    output[(y * SIZE.0 + x) as usize],
                    COLOR,
                    "the particle at ({x}, {y}) is not darkened"
                );
            }
        }
    }

    #[test]
    fn the_sky_is_untouched() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        for sky in [1.0, SKYBOX_DEPTH] {
            // A corner on the left half and sky on the right, so the sky
            // borders occluded geometry.
            let depth = scene_depth(
                &context.render_context,
                |x, _| (x < 0.0).then(|| 4.0 / (1.0 + (x + 0.3).abs())),
                sky,
            );

            let output = run_ssao(&context, &ssao_with_radius(CORNER_RADIUS), &depth);

            for y in 0..SIZE.1 {
                for x in SIZE.0 / 2..SIZE.0 {
                    assert_eq!(
                        output[(y * SIZE.0 + x) as usize],
                        COLOR,
                        "sky at ({x}, {y}), depth {sky}, is unchanged"
                    );
                }
            }
            assert!(
                visibility(&output).iter().any(|value| *value < 0.9),
                "the geometry beside the sky is occluded"
            );
        }
    }

    #[test]
    fn with_ssao_disabled_the_plan_is_unchanged() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let mut builder = SceneBuilder::new(context.registries.clone());
        let volume = builder.spawn("volume");
        builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
        let scene = builder.finish();
        let mut renderer = SceneRenderer::new(
            &context,
            SceneRendererOptions {
                samples: 1,
                ..Default::default()
            },
            SIZE,
        )
        .expect("scene renderer should load its assets");
        renderer.render_scene_base(&camera(), &Default::default(), &scene, None, None);
        renderer.finalize_scene();

        let names: Vec<_> = renderer
            .post_process()
            .passes()
            .iter()
            .map(|pass| pass.name())
            .collect();
        assert_eq!(names.first(), Some(&"ssao"), "ssao runs first");
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
            },
            "with ssao off the frame is tone mapping alone and builds no depth"
        );
        assert_eq!(renderer.post_process().pool().allocated(), 0);
    }

    #[test]
    fn the_targets_are_half_the_chain_size_and_freed_on_release() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let mut pass = SsaoPass::new(&context).expect("ssao loads");
        assert_eq!(pass.target_size(), None, "nothing is allocated up front");

        let record = |pass: &mut SsaoPass, size: (u32, u32)| {
            let input = texture(&render_context, size, wgpu::TextureFormat::Rgba16Float);
            let output = texture(&render_context, size, wgpu::TextureFormat::Rgba16Float);
            let depth = texture(&render_context, size, wgpu::TextureFormat::Depth32Float);
            let pass_context = PassContext {
                render_context: &render_context,
                size,
                camera: planes(),
            };
            pass.prepare(&pass_context, &ssao());
            let mut encoder = render_context
                .device()
                .create_command_encoder(&Default::default());
            pass.record(
                &pass_context,
                &mut encoder,
                PassIo {
                    input: &input,
                    output: &output,
                    depth: Some(&depth),
                    particle_depth: Some(&depth),
                },
            )
            .expect("the pass's shaders build");
            render_context.queue().submit(Some(encoder.finish()));
            pass.target_size()
        };

        assert_eq!(record(&mut pass, (32, 16)), Some((16, 8)));
        assert_eq!(
            record(&mut pass, (5, 3)),
            Some((3, 2)),
            "odd sizes round up and a new chain size refits the targets"
        );
        pass.release_targets();
        assert_eq!(pass.target_size(), None, "releasing frees the targets");
    }

    #[test]
    fn ssao_is_off_by_default_and_a_partial_component_loads_with_defaults() {
        assert_eq!(PostProcessSettings::default().ambient_occlusion, None);
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
