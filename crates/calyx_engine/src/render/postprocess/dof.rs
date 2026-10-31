//! The depth of field pass: blurs the scene by distance from a focus plane.

use super::{
    PassContext, PassInputs, PassIo, PostProcessPass, PostProcessSettings, PostProcessStage,
    SceneInput,
};
use crate as calyx_engine;
use crate::assets::texture::Texture;
use crate::component::{Component, ReflectComponent};
use crate::context::ReadOnlyAssetContext;
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{CachedBindGroup, PipelineOptions, RenderUtils, SceneRendererError, Shader};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui_wgpu::wgpu;
use serde::{Deserialize, Serialize};

/// Depth of field: the scene blurs with distance from a focus plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "ac9e495b-1859-42d8-b1c5-0d35f113c089"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Depth Of Field", category = "Post Processing")]
#[serde(default)]
pub struct DepthOfFieldSettings {
    /// View distance, in world units, that is sharp.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub focus_distance: f32,
    /// Distance in front of and behind the focus plane, in world units, over
    /// which the blur grows from none to `max_blur`.
    #[reflect_attr(min = 0.01, speed = 0.05)]
    pub focus_range: f32,
    /// Largest blur radius, in pixels.
    #[reflect_attr(min = 0.0, max = 32.0, speed = 0.1)]
    pub max_blur: f32,
}

impl DepthOfFieldSettings {
    /// The largest `max_blur` the pass honors, in pixels. The gather's tap
    /// count grows with the radius, so it is capped.
    pub const MAX_BLUR_LIMIT: f32 = 32.0;

    /// The blur radius the pass uses: `max_blur` within
    /// `[0, MAX_BLUR_LIMIT]`.
    pub fn blur_radius(&self) -> f32 {
        self.max_blur.clamp(0.0, Self::MAX_BLUR_LIMIT)
    }

    /// Spacing of the gather's spiral. Taps number about
    /// `radius^2 / (2 * spacing)`, so the spacing grows past 8 pixels to
    /// keep the count at most about 256.
    fn radius_scale(&self) -> f32 {
        (self.blur_radius() / 16.0).max(0.5)
    }
}

impl Component for DepthOfFieldSettings {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for DepthOfFieldSettings {
    fn default() -> Self {
        Self {
            focus_distance: 10.0,
            focus_range: 10.0,
            max_blur: 8.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct DofUniform {
    near_plane: f32,
    far_plane: f32,
    focus_distance: f32,
    focus_range: f32,
    max_blur: f32,
    radius_scale: f32,
    _padding: [f32; 2],
}

/// Depth of field in the `Hdr` stage. It computes each pixel's signed circle
/// of confusion from linear depth into a texture it owns, then gathers a disc
/// blur scaled by it, for both the near and the far field. It runs while the
/// settings hold a [`DepthOfFieldSettings`].
pub struct DepthOfFieldPass {
    coc_shader: Ref<Shader>,
    blur_shader: Ref<Shader>,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    coc_target: Option<Texture>,
    coc_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView)>,
    blur_bind_group: CachedBindGroup<(wgpu::BindGroupLayout, wgpu::TextureView, wgpu::TextureView)>,
}

impl DepthOfFieldPass {
    /// Shader asset that computes the circle of confusion.
    pub const COC_SHADER: &'static str = "shaders/postprocess/dof_coc";
    /// Shader asset that gathers the blur.
    pub const BLUR_SHADER: &'static str = "shaders/postprocess/dof_blur";
    /// Format of the circle of confusion texture: one signed radius in
    /// pixels, filterable.
    pub const COC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

    /// Creates the pass, or reports why its shaders did not load.
    pub fn new(context: &ReadOnlyAssetContext) -> Result<Self, SceneRendererError> {
        let coc_shader = SceneRendererError::load::<Shader>(context, Self::COC_SHADER)?;
        let blur_shader = SceneRendererError::load::<Shader>(context, Self::BLUR_SHADER)?;
        let device = context.render_context.device();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dof_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dof_uniform_buffer"),
            size: size_of::<DofUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            coc_shader,
            blur_shader,
            sampler,
            uniform_buffer,
            coc_target: None,
            coc_bind_group: Default::default(),
            blur_bind_group: Default::default(),
        })
    }

    /// Size of the circle of confusion texture the pass holds, if any.
    pub fn coc_target_size(&self) -> Option<(u32, u32)> {
        self.coc_target.as_ref().map(|target| {
            let size = target.descriptor.size;
            (size.width, size.height)
        })
    }

    /// The circle of confusion texture at `context`'s size, created or
    /// recreated as needed.
    fn coc_target(&mut self, context: &PassContext) -> &Texture {
        if self.coc_target_size() != Some(context.size) {
            self.release_targets();
            self.coc_target = Some(Texture::new(
                context.render_context.clone(),
                &wgpu::TextureDescriptor {
                    label: Some("dof_coc"),
                    size: wgpu::Extent3d {
                        width: context.size.0,
                        height: context.size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: Self::COC_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                None,
                None,
                false,
            ));
        }
        self.coc_target
            .as_ref()
            .expect("the target was just created")
    }
}

impl PostProcessPass for DepthOfFieldPass {
    fn name(&self) -> &str {
        "depth_of_field"
    }

    fn stage(&self) -> PostProcessStage {
        PostProcessStage::Hdr
    }

    fn inputs(&self) -> PassInputs {
        SceneInput::Depth.into()
    }

    fn enabled(&self, settings: &PostProcessSettings) -> bool {
        settings.depth_of_field.is_some()
    }

    fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
        let Some(dof) = &settings.depth_of_field else {
            return;
        };
        let uniform = DofUniform {
            near_plane: context.camera.near,
            far_plane: context.camera.far,
            focus_distance: dof.focus_distance,
            focus_range: dof.focus_range,
            max_blur: dof.blur_radius(),
            radius_scale: dof.radius_scale(),
            _padding: Default::default(),
        };
        context.render_context.queue().write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&uniform),
        );
    }

    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo) {
        let depth = io
            .depth
            .expect("the chain gives depth to a pass that asks for it");
        let device = context.render_context.device();
        let coc_target = self.coc_target(context).view.clone();
        let uniform_buffer = &self.uniform_buffer;

        // Circle of confusion.
        let shader = self.coc_shader.read();
        let pipeline = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(Self::COC_FORMAT))
                .build(),
        );
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let bind_group =
            self.coc_bind_group
                .get_or_create((layout.clone(), depth.view.clone()), || {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("dof_coc_bind_group"),
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
            RenderUtils::fullscreen_pass(encoder, &coc_target, "Depth Of Field CoC");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
        drop(render_pass);

        // Gather blur.
        let shader = self.blur_shader.read();
        let pipeline = shader.pipeline(
            &PipelineOptions::fullscreen_builder()
                .target(RenderUtils::color_default(io.output.descriptor.format))
                .build(),
        );
        let layout = shader.bind_group_layouts[0].clone();
        drop(shader);
        let sampler = &self.sampler;
        let bind_group = self.blur_bind_group.get_or_create(
            (layout.clone(), io.input.view.clone(), coc_target.clone()),
            || {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("dof_blur_bind_group"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&coc_target),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
            },
        );
        let mut render_pass =
            RenderUtils::fullscreen_pass(encoder, &io.output.view, "Depth Of Field Blur");
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut render_pass);
    }

    fn release_targets(&mut self) {
        self.coc_target = None;
        self.coc_bind_group.clear();
        self.blur_bind_group.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::postprocess::{
        CameraPlanes, ChainPlan, ChainTargets, PassTarget, PostProcessChain, TonemapPass,
    };
    use crate::render::scene_renderer::half_to_f32;
    use crate::render::{Camera, RenderContext};
    use crate::test_support::{test_adapter, TestAssets};
    use std::sync::{Arc, Mutex};

    const SIZE: u32 = 32;
    const CAMERA: CameraPlanes = CameraPlanes {
        near: 0.1,
        far: 100.0,
        // Depth of field only reads the planes.
        tan_half_fov: [1.0, 1.0],
    };
    const FOCUS: f32 = 10.0;
    const RANGE: f32 = 10.0;
    const MAX_BLUR: f32 = 6.0;

    /// A `SIZE` square image, row-major.
    type Image = Vec<[f32; 4]>;

    fn dof_on() -> PostProcessSettings {
        PostProcessSettings {
            depth_of_field: Some(DepthOfFieldSettings {
                focus_distance: FOCUS,
                focus_range: RANGE,
                max_blur: MAX_BLUR,
            }),
            ..Default::default()
        }
    }

    /// Lets a test inspect a pass the chain owns.
    struct Shared(Arc<Mutex<DepthOfFieldPass>>);

    impl PostProcessPass for Shared {
        fn name(&self) -> &str {
            "depth_of_field"
        }
        fn stage(&self) -> PostProcessStage {
            self.0.lock().unwrap().stage()
        }
        fn inputs(&self) -> PassInputs {
            self.0.lock().unwrap().inputs()
        }
        fn enabled(&self, settings: &PostProcessSettings) -> bool {
            self.0.lock().unwrap().enabled(settings)
        }
        fn prepare(&mut self, context: &PassContext, settings: &PostProcessSettings) {
            self.0.lock().unwrap().prepare(context, settings);
        }
        fn record(
            &mut self,
            context: &PassContext,
            encoder: &mut wgpu::CommandEncoder,
            io: PassIo,
        ) {
            self.0.lock().unwrap().record(context, encoder, io);
        }
        fn release_targets(&mut self) {
            self.0.lock().unwrap().release_targets();
        }
    }

    fn color_target(render_context: &Arc<RenderContext>, size: u32) -> Texture {
        Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("dof_test_color"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
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

    fn depth_target(render_context: &Arc<RenderContext>, size: u32, samples: u32) -> Texture {
        Texture::new(
            render_context.clone(),
            &wgpu::TextureDescriptor {
                label: Some("dof_test_depth"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            None,
            None,
            false,
        )
    }

    const DEPTH_FILL_SHADER: &str = "
    struct Split { x: f32, left: f32, right: f32, _padding: f32 };
    @group(0) @binding(0) var<uniform> split: Split;
    @vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
        let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
        return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
    }
    @fragment fn fs_main(@builtin(position) position: vec4f) -> @builtin(frag_depth) f32 {
        return select(split.right, split.left, position.x < split.x);
    }
    ";

    /// Fills `depth` with the depth of view distance `left` left of column
    /// `split`, and of `right` from it on.
    fn fill_depth(
        render_context: &Arc<RenderContext>,
        depth: &Texture,
        split: u32,
        left: f32,
        right: f32,
    ) {
        let device = render_context.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dof_test_depth_fill"),
            source: wgpu::ShaderSource::Wgsl(DEPTH_FILL_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dof_test_depth_fill"),
            layout: None,
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
            multisample: wgpu::MultisampleState {
                count: depth.descriptor.sample_count,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });
        let uniform = [
            split as f32,
            CAMERA.depth_of(left),
            CAMERA.depth_of(right),
            0.0,
        ];
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dof_test_depth_fill"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        render_context
            .queue()
            .write_buffer(&buffer, 0, bytemuck::cast_slice(&uniform));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dof_test_depth_fill"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dof_test_depth_fill"),
            color_attachments: &[],
            depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
                &depth.view,
                1.0,
                None,
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut pass);
        drop(pass);
        render_context.queue().submit(Some(encoder.finish()));
    }

    /// `value` as half-float bits. Exact for the values these tests use.
    fn f32_to_half(value: f32) -> u16 {
        if value == 0.0 {
            return 0;
        }
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
        let fraction = ((bits >> 13) & 0x3ff) as u16;
        sign | ((exponent as u16) << 10) | fraction
    }

    fn upload(render_context: &RenderContext, texture: &Texture, image: &Image) {
        let bytes: Vec<u8> = image
            .iter()
            .flat_map(|texel| texel.map(f32_to_half))
            .flat_map(u16::to_le_bytes)
            .collect();
        render_context.queue().write_texture(
            texture.texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 8),
                rows_per_image: Some(SIZE),
            },
            texture.descriptor.size,
        );
    }

    fn read(render_context: &RenderContext, texture: &Texture) -> Image {
        let device = render_context.device();
        // A row of `SIZE` half-float texels is exactly the copy alignment.
        let row = SIZE * 8;
        assert_eq!(row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dof_test_readback"),
            size: (row * SIZE) as u64,
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
                    bytes_per_row: Some(row),
                    rows_per_image: Some(SIZE),
                },
            },
            texture.descriptor.size,
        );
        render_context.queue().submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
        let _ = device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let image = data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|texel| {
                std::array::from_fn(|channel| {
                    half_to_f32(u16::from_le_bytes([
                        texel[channel * 2],
                        texel[channel * 2 + 1],
                    ]))
                })
            })
            .collect();
        drop(data);
        buffer.unmap();
        image
    }

    /// Vertical one-pixel stripes of white and black, with alpha 0.5 on the
    /// black ones.
    fn stripes() -> Image {
        (0..SIZE * SIZE)
            .map(|index| {
                if index % 2 == 0 {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.0, 0.0, 0.0, 0.5]
                }
            })
            .collect()
    }

    /// Runs a chain holding only depth of field over `image`, with `depth`
    /// as the scene depth.
    fn run(context: &ReadOnlyAssetContext, image: &Image, depth: &Texture) -> Image {
        let render_context = context.render_context.clone();
        let mut chain = PostProcessChain::new(context).expect("chain shaders load");
        chain.push(Box::new(DepthOfFieldPass::new(context).expect("dof loads")));
        chain.set_camera(CAMERA);
        let source = color_target(&render_context, SIZE);
        let output = color_target(&render_context, SIZE);
        upload(&render_context, &source, image);
        let mut encoder = render_context
            .device()
            .create_command_encoder(&Default::default());
        let plan = chain.run(
            &render_context,
            &mut encoder,
            &dof_on(),
            ChainTargets {
                source: &source,
                output: &output,
                depth,
            },
        );
        render_context.queue().submit(Some(encoder.finish()));
        assert_eq!(plan.passes.len(), 1, "depth of field runs");
        assert!(plan.depth, "depth of field reads depth");
        read(&render_context, &output)
    }

    /// Mean absolute red difference between `a` and `b` over the pixels at
    /// least `MAX_BLUR` from every edge.
    fn interior_difference(a: &Image, b: &Image) -> f32 {
        let margin = MAX_BLUR as usize;
        let size = SIZE as usize;
        let interior: Vec<f32> = (margin..size - margin)
            .flat_map(|y| (margin..size - margin).map(move |x| y * size + x))
            .map(|index| (a[index][0] - b[index][0]).abs())
            .collect();
        interior.iter().sum::<f32>() / interior.len() as f32
    }

    fn uniform_depth(context: &ReadOnlyAssetContext, distance: f32, samples: u32) -> Texture {
        let render_context = &context.render_context;
        let depth = depth_target(render_context, SIZE, samples);
        fill_depth(render_context, &depth, 0, distance, distance);
        depth
    }

    #[test]
    fn camera_planes_invert_the_render_projection() {
        let camera = Camera::new(1.0, 60f32.to_radians(), CAMERA.near, CAMERA.far);
        for distance in [0.5, 1.0, 10.0, 60.0, 99.0] {
            let clip = camera.projection * nalgebra_glm::vec4(0.0, 0.0, distance, 1.0);
            let depth = clip.z / clip.w;
            assert!(
                (CAMERA.depth_of(distance) - depth).abs() < 1e-5,
                "depth_of({distance}) matches the projection"
            );
            let linear = CAMERA.linear_depth(depth);
            assert!(
                (linear - distance).abs() < distance * 1e-3,
                "linear_depth({depth}) = {linear}, expected {distance}"
            );
        }
    }

    #[test]
    fn depth_of_field_is_off_by_default_and_a_partial_component_loads_with_defaults() {
        assert_eq!(PostProcessSettings::default().depth_of_field, None);
        let defaults = DepthOfFieldSettings::default();
        let partial = r#"{"focus_distance":4.0}"#;
        let dof: DepthOfFieldSettings = serde_json::from_str(partial).expect("partial loads");
        assert_eq!(dof.focus_distance, 4.0);
        assert_eq!(dof.max_blur, defaults.max_blur);
    }

    #[test]
    fn pixels_at_the_focus_distance_are_unchanged() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let image = stripes();
        let depth = uniform_depth(&context, FOCUS, 1);

        let output = run(&context, &image, &depth);

        assert!(output == image, "in-focus pixels keep their exact value");
    }

    /// Black on the left half, white on the right.
    fn step_edge() -> Image {
        (0..SIZE * SIZE)
            .map(|index| {
                if index % SIZE < SIZE / 2 {
                    [0.0, 0.0, 0.0, 1.0]
                } else {
                    [1.0, 1.0, 1.0, 1.0]
                }
            })
            .collect()
    }

    /// How many pixels of the middle row the edge of [`step_edge`] spreads
    /// over: those neither black nor white.
    fn edge_width(output: &Image) -> usize {
        let row = (SIZE / 2 * SIZE) as usize;
        output[row..row + SIZE as usize]
            .iter()
            .filter(|texel| texel[0] > 0.01 && texel[0] < 0.99)
            .count()
    }

    #[test]
    fn far_pixels_blur_more_than_pixels_near_focus() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let image = step_edge();
        let width = |distance: f32| {
            edge_width(&run(
                &context,
                &image,
                &uniform_depth(&context, distance, 1),
            ))
        };

        // Circles of confusion of 1.2 and 6 pixels.
        let near_focus = width(FOCUS + 2.0);
        let far = width(60.0);
        // In front of the focus plane: -4.8 pixels.
        let near_field = width(2.0);

        assert!(near_focus > 0, "slightly out of focus still blurs");
        assert!(
            far > near_focus * 2,
            "far pixels blur over {far} pixels, more than the {near_focus} near focus"
        );
        assert!(
            near_field > near_focus * 2,
            "the near field blurs too, over {near_field} pixels against {near_focus}"
        );
    }

    #[test]
    fn a_blurred_foreground_spreads_over_focus_and_a_blurred_background_does_not() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let half = SIZE / 2;
        let image = step_edge();
        let at = |output: &Image, x: u32| output[(half * SIZE + x) as usize][0];

        // Left half far behind the focus plane, right half in focus.
        let depth = depth_target(&render_context, SIZE, 1);
        fill_depth(&render_context, &depth, half, 60.0, FOCUS);
        let behind = run(&context, &image, &depth);
        assert_eq!(
            at(&behind, half),
            1.0,
            "the blurred background stays behind the sharp edge"
        );
        assert!(at(&behind, half - 1) > 0.0, "the background itself blurs");

        // Left half in front of the focus plane.
        fill_depth(&render_context, &depth, half, 2.0, FOCUS);
        let in_front = run(&context, &image, &depth);
        assert!(
            at(&in_front, half) < 1.0,
            "the blurred foreground spreads over the sharp pixels behind it"
        );
        assert_eq!(
            at(&in_front, SIZE - 2),
            1.0,
            "sharp pixels out of the foreground's reach are unchanged"
        );
    }

    #[test]
    fn the_pass_reads_depth_at_one_and_four_samples() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let image = stripes();
        let mut outputs = Vec::new();
        for samples in [1, 4] {
            if !test_adapter()
                .get_texture_format_features(wgpu::TextureFormat::Depth32Float)
                .flags
                .sample_count_supported(samples)
            {
                eprintln!("skipping {samples} samples: unsupported by the test adapter");
                continue;
            }
            let focus = run(&context, &image, &uniform_depth(&context, FOCUS, samples));
            assert!(focus == image, "in focus at {samples} samples");
            let far = run(&context, &image, &uniform_depth(&context, 60.0, samples));
            assert!(
                interior_difference(&far, &image) > 0.1,
                "far pixels blur at {samples} samples"
            );
            outputs.push(far);
        }
        if let [one, four] = outputs.as_slice() {
            assert!(one == four, "four samples blur as one does");
        }
    }

    #[test]
    fn with_depth_of_field_disabled_the_plan_is_unchanged_and_nothing_is_allocated() {
        let assets = TestAssets::engine();
        let context = assets.asset_context();
        let context = context.lock_read();
        let render_context = context.render_context.clone();
        let dof = Arc::new(Mutex::new(
            DepthOfFieldPass::new(&context).expect("dof loads"),
        ));
        let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
        chain.push(Box::new(TonemapPass::new(&context).expect("tonemap loads")));
        chain.push(Box::new(Shared(dof.clone())));
        let names = |chain: &PostProcessChain, plan: &ChainPlan| {
            plan.passes
                .iter()
                .map(|planned| chain.passes()[planned.pass].name().to_string())
                .collect::<Vec<_>>()
        };

        let plan = ChainPlan::build(chain.passes(), &Default::default());
        assert_eq!(names(&chain, &plan), ["tonemap"]);
        assert_eq!(plan.passes[0].input, PassTarget::Source);
        assert_eq!(plan.passes[0].output, PassTarget::Output);
        assert_eq!(plan.intermediates, 0);
        assert!(!plan.depth, "nothing reads depth");

        let plan = ChainPlan::build(chain.passes(), &dof_on());
        assert_eq!(
            names(&chain, &plan),
            ["depth_of_field", "tonemap"],
            "depth of field runs in the Hdr stage, before tone mapping"
        );

        let mut frame = |settings: &PostProcessSettings, size: u32| {
            let source = color_target(&render_context, size);
            let output = color_target(&render_context, size);
            let depth = depth_target(&render_context, size, 1);
            let mut encoder = render_context
                .device()
                .create_command_encoder(&Default::default());
            chain.run(
                &render_context,
                &mut encoder,
                settings,
                ChainTargets {
                    source: &source,
                    output: &output,
                    depth: &depth,
                },
            );
            render_context.queue().submit(Some(encoder.finish()));
        };
        let allocated = || dof.lock().unwrap().coc_target_size();

        frame(&Default::default(), SIZE);
        assert_eq!(allocated(), None, "nothing is allocated while disabled");
        frame(&dof_on(), SIZE);
        assert_eq!(allocated(), Some((SIZE, SIZE)), "sized from the chain");
        frame(&dof_on(), SIZE * 2);
        assert_eq!(
            allocated(),
            Some((SIZE * 2, SIZE * 2)),
            "resized with the chain"
        );
        frame(&Default::default(), SIZE);
        assert_eq!(allocated(), None, "freed once disabled");
        frame(&dof_on(), SIZE);
        assert!(allocated().is_some());
        chain.clear_targets();
        assert_eq!(allocated(), None, "freed on resize");
    }
}
