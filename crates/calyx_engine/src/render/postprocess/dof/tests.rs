use super::*;
use crate::component::ComponentDepthOfField;
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
    PostProcessSettings::default().with(ComponentDepthOfField {
        focus_distance: FOCUS,
        focus_range: RANGE,
        max_blur: MAX_BLUR,
    })
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
    ) -> Result<(), NoPipeline> {
        self.0.lock().unwrap().record(context, encoder, io)
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
            particle_depth: None,
            normals: None,
            albedo: None,
            gbuffer_depth: None,
            material: None,
        },
    );
    render_context.queue().submit(Some(encoder.finish()));
    assert_eq!(plan.passes.len(), 1, "depth of field runs");
    assert!(plan.particle_depth, "depth of field reads particle depth");
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
    assert_eq!(
        PostProcessSettings::default()
            .get::<ComponentDepthOfField>()
            .copied(),
        None
    );
    let defaults = ComponentDepthOfField::default();
    let partial = r#"{"focus_distance":4.0}"#;
    let dof: ComponentDepthOfField = serde_json::from_str(partial).expect("partial loads");
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
                particle_depth: None,
                normals: None,
                albedo: None,
                gbuffer_depth: None,
                material: None,
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
