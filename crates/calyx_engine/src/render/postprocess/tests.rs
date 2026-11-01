use super::*;
use crate::assets::texture::Texture;
use crate::component::{
    Component, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentColorGrading, ComponentDepthOfField, ComponentGlobalPostProcessVolume,
    ComponentTonemap, ComponentVignette, TonemappingPreset,
};
use crate::render::scene_renderer::{half_to_f32, linear_to_srgb};
use crate::render::{
    Camera, ReadbackPoll, RenderContext, RenderUtils, SceneRenderer, SceneRendererOptions,
};
use crate::scene::{GameObject, Scene};
use crate::test_support::{test_adapter, test_registries, SceneBuilder, TestAssets};
use egui::Color32;
use egui_wgpu::wgpu;
use std::sync::{Arc, Mutex};

const SIZE: u32 = 4;

/// Hill's ACES fit, the reference the tone-map shader must match.
fn aces_reference(color: [f32; 3]) -> [f32; 3] {
    const INPUT: [[f32; 3]; 3] = [
        [0.59719, 0.35458, 0.04823],
        [0.07600, 0.90834, 0.01566],
        [0.02840, 0.13383, 0.83777],
    ];
    const OUTPUT: [[f32; 3]; 3] = [
        [1.60475, -0.53108, -0.07367],
        [-0.10208, 1.10813, -0.00605],
        [-0.00327, -0.07276, 1.07602],
    ];
    let multiply = |matrix: [[f32; 3]; 3], v: [f32; 3]| {
        matrix.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
    };
    let fit =
        |v: f32| (v * (v + 0.0245786) - 0.000090537) / (v * (0.983729 * v + 0.4329510) + 0.238081);
    multiply(OUTPUT, multiply(INPUT, color).map(fit)).map(|v| v.clamp(0.0, 1.0))
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    // Rgba16Float keeps about three decimal digits.
    assert!(
        (actual - expected).abs() <= 2e-3,
        "{what}: expected {expected}, got {actual}"
    );
}

/// A pass for planning tests: it is never recorded.
struct StubPass {
    name: &'static str,
    stage: PostProcessStage,
    enabled: bool,
    inputs: PassInputs,
}

impl StubPass {
    fn boxed(name: &'static str, stage: PostProcessStage) -> Box<dyn PostProcessPass> {
        Box::new(Self {
            name,
            stage,
            enabled: true,
            inputs: PassInputs::empty(),
        })
    }
}

impl PostProcessPass for StubPass {
    fn name(&self) -> &str {
        self.name
    }
    fn stage(&self) -> PostProcessStage {
        self.stage
    }
    fn inputs(&self) -> PassInputs {
        self.inputs
    }
    fn enabled(&self, _settings: &PostProcessSettings) -> bool {
        self.enabled
    }
    fn prepare(&mut self, _context: &PassContext, _settings: &PostProcessSettings) {}
    fn record(&mut self, _: &PassContext, _: &mut wgpu::CommandEncoder, _: PassIo) {
        unreachable!("planning tests never record");
    }
}

fn targets(plan: &ChainPlan) -> Vec<(PassTarget, PassTarget)> {
    plan.passes
        .iter()
        .map(|planned| (planned.input, planned.output))
        .collect()
}

#[test]
fn a_single_pass_reads_the_source_and_writes_the_output() {
    let passes = vec![StubPass::boxed("tonemap", PostProcessStage::Tonemap)];
    let plan = ChainPlan::build(&passes, &Default::default());
    assert_eq!(
        targets(&plan),
        vec![(PassTarget::Source, PassTarget::Output)]
    );
    assert_eq!(plan.intermediates, 0, "one pass needs no pool target");
}

#[test]
fn passes_between_source_and_output_ping_pong_two_pool_targets() {
    use IntermediateSlot::*;
    use PassTarget::*;
    let passes: Vec<_> = (0..4)
        .map(|_| StubPass::boxed("pass", PostProcessStage::Hdr))
        .collect();

    let plan = ChainPlan::build(&passes[..2], &Default::default());
    assert_eq!(
        targets(&plan),
        vec![(Source, Intermediate(A)), (Intermediate(A), Output)]
    );
    assert_eq!(plan.intermediates, 1);

    let plan = ChainPlan::build(&passes, &Default::default());
    assert_eq!(
        targets(&plan),
        vec![
            (Source, Intermediate(A)),
            (Intermediate(A), Intermediate(B)),
            (Intermediate(B), Intermediate(A)),
            (Intermediate(A), Output),
        ]
    );
    assert_eq!(
        plan.intermediates, 2,
        "four passes still alternate two targets"
    );
    for planned in &plan.passes {
        assert_ne!(
            planned.input, planned.output,
            "a pass never reads what it writes"
        );
    }
}

#[test]
fn disabled_passes_and_passes_without_their_inputs_are_left_out() {
    let disabled = Box::new(StubPass {
        name: "disabled",
        stage: PostProcessStage::Hdr,
        enabled: false,
        inputs: PassInputs::empty(),
    });
    let normals = Box::new(StubPass {
        name: "ssao",
        stage: PostProcessStage::Hdr,
        enabled: true,
        inputs: SceneInput::Normals.into(),
    });
    let velocity = Box::new(StubPass {
        name: "motion_blur",
        stage: PostProcessStage::Hdr,
        enabled: true,
        inputs: SceneInput::Velocity.into(),
    });
    let depth = Box::new(StubPass {
        name: "fog",
        stage: PostProcessStage::Hdr,
        enabled: true,
        inputs: SceneInput::Depth.into(),
    });
    let passes: Vec<Box<dyn PostProcessPass>> = vec![
        disabled,
        normals,
        velocity,
        depth,
        StubPass::boxed("tonemap", PostProcessStage::Tonemap),
    ];

    let plan = ChainPlan::build(&passes, &Default::default());

    assert_eq!(
        plan.passes
            .iter()
            .map(|planned| planned.pass)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert!(plan.depth, "the fog pass asked for depth");
    assert_eq!(
        plan.skipped,
        vec![
            PostProcessError::InputUnavailable {
                pass: "ssao".into(),
                input: SceneInput::Normals,
            },
            PostProcessError::InputUnavailable {
                pass: "motion_blur".into(),
                input: SceneInput::Velocity,
            },
        ]
    );
    assert_eq!(
        plan.skipped[0].to_string(),
        "post-process pass `ssao` needs normals, which the renderer does not produce yet"
    );
}

#[test]
fn passes_run_in_stage_order_then_insertion_order() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let mut chain = PostProcessChain::new(&context.lock_read()).expect("chain shaders load");
    chain.push(StubPass::boxed("vignette", PostProcessStage::Ldr));
    chain.push(StubPass::boxed("bloom", PostProcessStage::Hdr));
    chain.push(StubPass::boxed("tonemap", PostProcessStage::Tonemap));
    chain.push(StubPass::boxed("grain", PostProcessStage::Ldr));
    chain.push(StubPass::boxed("dof", PostProcessStage::Hdr));

    let names: Vec<_> = chain.passes().iter().map(|pass| pass.name()).collect();
    assert_eq!(names, ["bloom", "dof", "tonemap", "vignette", "grain"]);
}

// GPU tests.

fn color_target(render_context: &Arc<RenderContext>, label: &'static str) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        None,
        None,
        false,
    )
}

fn depth_target(render_context: &Arc<RenderContext>, samples: u32) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("test_scene_depth"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        None,
        None,
        false,
    )
}

fn clear_color(encoder: &mut wgpu::CommandEncoder, texture: &Texture, color: [f64; 4]) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("test_clear_color"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &texture.view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: color[0],
                    g: color[1],
                    b: color[2],
                    a: color[3],
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

fn clear_depth(encoder: &mut wgpu::CommandEncoder, texture: &Texture, depth: f32) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("test_clear_depth"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &texture.view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(depth),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

/// Reads the top-left texel of `texture`, as raw bytes of `bytes` length.
fn read_first_texel(
    render_context: &RenderContext,
    texture: &wgpu::Texture,
    aspect: wgpu::TextureAspect,
    bytes: usize,
) -> Vec<u8> {
    let device = render_context.device();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("post_process_test_readback"),
        size: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Default::default(),
            aspect,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range()[..bytes].to_vec();
    buffer.unmap();
    data
}

/// The top-left texel of an `Rgba16Float` texture.
fn read_rgba16f(render_context: &RenderContext, texture: &Texture) -> [f32; 4] {
    let bytes = read_first_texel(
        render_context,
        &texture.texture,
        wgpu::TextureAspect::All,
        8,
    );
    std::array::from_fn(|channel| {
        half_to_f32(u16::from_le_bytes([
            bytes[channel * 2],
            bytes[channel * 2 + 1],
        ]))
    })
}

const AFFINE_SHADER: &str = "
struct Affine { scale: f32, offset: f32, _padding: vec2f };
@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var<uniform> affine: Affine;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let texel = textureLoad(input_texture, vec2i(position.xy), 0);
    return vec4f(texel.rgb * affine.scale + affine.offset, texel.a);
}
";

/// What an [`AffinePass`] saw when it recorded.
#[derive(Default)]
struct Recorded {
    order: Vec<&'static str>,
    depth_samples: Vec<Option<u32>>,
    depth: Option<wgpu::Texture>,
}

/// Writes `input * scale + offset` to the color channels.
struct AffinePass {
    name: &'static str,
    stage: PostProcessStage,
    inputs: PassInputs,
    scale: f32,
    offset: f32,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    recorded: Arc<Mutex<Recorded>>,
}

impl AffinePass {
    fn boxed(
        render_context: &RenderContext,
        recorded: &Arc<Mutex<Recorded>>,
        name: &'static str,
        stage: PostProcessStage,
        scale: f32,
        offset: f32,
    ) -> Box<Self> {
        let device = render_context.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("affine_test_pass"),
            source: wgpu::ShaderSource::Wgsl(AFFINE_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("affine_test_pass"),
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
                targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("affine_test_uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Box::new(Self {
            name,
            stage,
            inputs: PassInputs::empty(),
            scale,
            offset,
            pipeline,
            uniform,
            recorded: recorded.clone(),
        })
    }

    fn reading_depth(mut self: Box<Self>) -> Box<Self> {
        self.inputs |= SceneInput::Depth;
        self
    }
}

impl PostProcessPass for AffinePass {
    fn name(&self) -> &str {
        self.name
    }
    fn stage(&self) -> PostProcessStage {
        self.stage
    }
    fn inputs(&self) -> PassInputs {
        self.inputs
    }
    fn enabled(&self, _settings: &PostProcessSettings) -> bool {
        true
    }
    fn prepare(&mut self, context: &PassContext, _settings: &PostProcessSettings) {
        context.render_context.queue().write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&[self.scale, self.offset, 0.0, 0.0]),
        );
    }
    fn record(&mut self, context: &PassContext, encoder: &mut wgpu::CommandEncoder, io: PassIo) {
        let mut recorded = self.recorded.lock().unwrap();
        recorded.order.push(self.name);
        recorded
            .depth_samples
            .push(io.depth.map(|depth| depth.descriptor.sample_count));
        if let Some(depth) = io.depth {
            recorded.depth = Some(depth.texture.clone());
        }
        let bind_group =
            context
                .render_context
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("affine_test_bind_group"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&io.input.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.uniform.as_entire_binding(),
                        },
                    ],
                });
        let mut pass = RenderUtils::fullscreen_pass(encoder, &io.output.view, self.name);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        RenderUtils::draw_fullscreen(&mut pass);
    }
}

/// Runs `chain` once from a source cleared to `source_color` and returns the
/// output's top-left texel and the plan it ran.
fn run_chain(
    render_context: &Arc<RenderContext>,
    chain: &mut PostProcessChain,
    settings: &PostProcessSettings,
    source_color: [f64; 4],
    depth: &Texture,
) -> ([f32; 4], ChainPlan) {
    let source = color_target(render_context, "test_source");
    let output = color_target(render_context, "test_output");
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    clear_color(&mut encoder, &source, source_color);
    let plan = chain.run(
        render_context,
        &mut encoder,
        settings,
        ChainTargets {
            source: &source,
            output: &output,
            depth,
            particle_depth: None,
        },
    );
    render_context.queue().submit(Some(encoder.finish()));
    let texel = read_rgba16f(render_context, &output);
    assert_close(
        read_rgba16f(render_context, &source)[0],
        source_color[0] as f32,
        "the chain must not write its source",
    );
    (texel, plan)
}

#[test]
fn passes_run_in_chain_order_through_reused_pool_targets() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let recorded = Arc::new(Mutex::new(Recorded::default()));
    let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
    // Pushed out of order; the three affine maps do not commute, so the
    // output tells the order they ran in.
    chain.push(AffinePass::boxed(
        &render_context,
        &recorded,
        "add_one",
        PostProcessStage::Ldr,
        1.0,
        1.0,
    ));
    chain.push(AffinePass::boxed(
        &render_context,
        &recorded,
        "double",
        PostProcessStage::Hdr,
        2.0,
        0.0,
    ));
    chain.push(AffinePass::boxed(
        &render_context,
        &recorded,
        "add_half",
        PostProcessStage::Hdr,
        1.0,
        0.5,
    ));
    let depth = depth_target(&render_context, 1);

    let (texel, plan) = run_chain(
        &render_context,
        &mut chain,
        &Default::default(),
        [1.0, 1.0, 1.0, 0.5],
        &depth,
    );

    assert_close(
        texel[0],
        (1.0 * 2.0 + 0.5) + 1.0,
        "double, add half, add one",
    );
    assert_eq!(texel[3], 0.5, "alpha passes through");
    assert_eq!(
        recorded.lock().unwrap().order,
        ["double", "add_half", "add_one"]
    );
    assert_eq!(plan.intermediates, 2);
    assert_eq!(chain.pool().allocated(), 2);

    // The next frame reuses the same targets.
    run_chain(
        &render_context,
        &mut chain,
        &Default::default(),
        [1.0, 1.0, 1.0, 0.5],
        &depth,
    );
    assert_eq!(
        chain.pool().allocated(),
        2,
        "targets are reused across frames"
    );
    chain.clear_targets();
    assert_eq!(chain.pool().allocated(), 0, "a resize frees the pool");
}

/// Whether the test device can render `format` at `samples`.
fn supports_samples(format: wgpu::TextureFormat, samples: u32) -> bool {
    test_adapter()
        .get_texture_format_features(format)
        .flags
        .sample_count_supported(samples)
}

#[test]
fn passes_that_read_depth_get_single_sample_scene_depth() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();

    for samples in [1, 4] {
        if !supports_samples(wgpu::TextureFormat::Depth32Float, samples) {
            eprintln!("skipping {samples} samples: unsupported by the test adapter");
            continue;
        }
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
        chain.push(
            AffinePass::boxed(
                &render_context,
                &recorded,
                "fog",
                PostProcessStage::Hdr,
                1.0,
                0.0,
            )
            .reading_depth(),
        );
        chain.push(AffinePass::boxed(
            &render_context,
            &recorded,
            "tonemap",
            PostProcessStage::Tonemap,
            1.0,
            0.0,
        ));
        let depth = depth_target(&render_context, samples);
        let mut encoder = render_context
            .device()
            .create_command_encoder(&Default::default());
        clear_depth(&mut encoder, &depth, 0.25);
        render_context.queue().submit(Some(encoder.finish()));

        run_chain(
            &render_context,
            &mut chain,
            &Default::default(),
            [0.0; 4],
            &depth,
        );

        let recorded = recorded.lock().unwrap();
        assert_eq!(
            recorded.depth_samples,
            [Some(1), None],
            "only the pass that asked for depth gets it, single-sample, at {samples} samples"
        );
        let copy = recorded.depth.as_ref().expect("the fog pass saw depth");
        let bytes = read_first_texel(&render_context, copy, wgpu::TextureAspect::DepthOnly, 4);
        assert_eq!(
            f32::from_le_bytes(bytes.try_into().unwrap()),
            0.25,
            "depth holds the scene's depth at {samples} samples"
        );
    }
}

#[test]
fn tone_mapping_matches_the_aces_fit_and_keeps_alpha() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
    chain.push(Box::new(TonemapPass::new(&context).expect("tonemap loads")));
    let depth = depth_target(&render_context, 1);

    // Reference values of the fit for gray inputs, computed independently.
    for (input, expected) in [(0.18, 0.10559), (1.0, 0.61912), (4.0, 0.90901)] {
        let (texel, plan) = run_chain(
            &render_context,
            &mut chain,
            &Default::default(),
            [input, input, input, 0.25],
            &depth,
        );
        for channel in 0..3 {
            assert_close(texel[channel], expected, &format!("ACES({input})"));
        }
        assert_eq!(texel[3], 0.25, "tone mapping keeps alpha");
        assert_eq!(plan.passes.len(), 1);
    }

    let (texel, _) = run_chain(
        &render_context,
        &mut chain,
        &Default::default(),
        [2.0, 0.5, 0.1, 0.0],
        &depth,
    );
    let expected = aces_reference([2.0, 0.5, 0.1]);
    for channel in 0..3 {
        assert_close(
            texel[channel],
            expected[channel],
            "ACES of a saturated color",
        );
    }
    assert_eq!(texel[3], 0.0, "transparent stays transparent");

    // Exposure scales the input by 2^EV before the curve.
    let settings = PostProcessSettings {
        tonemapping: ComponentTonemap {
            exposure_ev: 2.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let (texel, _) = run_chain(
        &render_context,
        &mut chain,
        &settings,
        [1.0, 1.0, 1.0, 1.0],
        &depth,
    );
    assert_close(texel[0], 0.90901, "ACES(1.0 * 2^2)");
    assert_eq!(
        chain.pool().allocated(),
        0,
        "tone mapping alone allocates no intermediate target"
    );
}

/// A volume of `priority` whose tone-mapping component sets `exposure_ev`,
/// with whether the scene holds the volume enabled.
#[derive(Clone, Copy)]
struct VolumeSpec {
    enabled: bool,
    priority: i32,
    exposure_ev: f32,
}

fn volume(enabled: bool, priority: i32, exposure_ev: f32) -> VolumeSpec {
    VolumeSpec {
        enabled,
        priority,
        exposure_ev,
    }
}

/// Adds a game object to `scene` for each of `volumes`, holding the volume
/// and its tone-mapping component, and returns them in order.
fn spawn_volumes(scene: &mut Scene, volumes: &[VolumeSpec]) -> Vec<GameObject> {
    volumes
        .iter()
        .map(|spec| {
            let game_object = scene.create(None, None);
            scene.add_component(
                game_object,
                ComponentGlobalPostProcessVolume::new(spec.priority),
            );
            scene.add_component(
                game_object,
                ComponentTonemap {
                    exposure_ev: spec.exposure_ev,
                    ..Default::default()
                },
            );
            scene.set_component_enabled::<ComponentGlobalPostProcessVolume>(
                game_object,
                spec.enabled,
            );
            game_object
        })
        .collect()
}

/// [`select_volume`] over `(enabled, priority)` pairs, picking an index.
fn select(volumes: &[(bool, i32)]) -> VolumeSelection<usize> {
    select_volume(
        volumes
            .iter()
            .enumerate()
            .map(|(index, (enabled, priority))| (*enabled, *priority, index)),
    )
}

#[test]
fn the_enabled_volume_with_the_highest_priority_is_selected() {
    let selection = select(&[(true, 1), (false, 9), (true, 3), (true, 2)]);
    assert_eq!(selection.selected, Some(2));
    assert_eq!(selection.tie, None);
}

#[test]
fn a_priority_tie_selects_the_first_found_and_reports_it() {
    let selection = select(&[(true, 2), (true, 2), (true, 0)]);
    assert_eq!(selection.selected, Some(0));
    assert_eq!(selection.tie, Some(2));

    // A higher priority after a tie wins outright.
    let selection = select(&[(true, 2), (true, 2), (true, 5)]);
    assert_eq!(selection.selected, Some(2));
    assert_eq!(selection.tie, None);
}

#[test]
fn no_enabled_volume_selects_nothing_and_the_defaults_tone_map_alone() {
    assert_eq!(select(&[]).selected, None);
    let selection = select(&[(false, 5)]);
    assert_eq!(selection.selected, None);
    assert_eq!(selection.tie, None);

    let defaults = PostProcessSettings::default();
    assert_eq!(defaults.tonemapping.preset, TonemappingPreset::Aces);
    assert_eq!(defaults.tonemapping.exposure_ev, 0.0);
    assert_eq!(
        defaults,
        PostProcessSettings {
            tonemapping: ComponentTonemap::default(),
            ambient_occlusion: None,
            depth_of_field: None,
            bloom: None,
            color_grading: None,
            vignette: None,
            anti_aliasing: None,
        },
        "every effect but tone mapping is off"
    );
}

#[test]
fn a_volume_without_effect_components_resolves_to_tone_mapping_alone() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let game_object = scene.create(None, None);
    scene.add_component(game_object, ComponentGlobalPostProcessVolume::default());
    assert_eq!(
        PostProcessSettings::selected_volume(&scene),
        Some(game_object)
    );
    assert_eq!(
        PostProcessSettings::resolve(&scene),
        PostProcessSettings::default()
    );
}

#[test]
fn effects_are_gathered_from_the_selected_volumes_game_object_only() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let objects = spawn_volumes(&mut scene, &[volume(true, 1, 1.0), volume(true, 4, -1.0)]);
    let (low, high) = (objects[0], objects[1]);
    let bloom = ComponentBloom {
        intensity: 0.3,
        ..Default::default()
    };
    scene.add_component(low, bloom);
    scene.add_component(high, ComponentAntiAliasing::default());
    // An effect on an object without a volume is never gathered.
    let stray = scene.create(None, None);
    scene.add_component(stray, ComponentVignette::default());

    let settings = PostProcessSettings::resolve(&scene);
    assert_eq!(settings.tonemapping.exposure_ev, -1.0);
    assert_eq!(
        settings.anti_aliasing,
        Some(ComponentAntiAliasing::default())
    );
    assert_eq!(settings.bloom, None, "the lower volume's bloom is not used");
    assert_eq!(settings.vignette, None);

    // Disabling the winning volume hands the frame to the other one.
    scene.set_component_enabled::<ComponentGlobalPostProcessVolume>(high, false);
    let settings = PostProcessSettings::resolve(&scene);
    assert_eq!(settings.tonemapping.exposure_ev, 1.0);
    assert_eq!(settings.bloom, Some(bloom));
    assert_eq!(settings.anti_aliasing, None);
}

#[test]
fn a_disabled_tone_mapping_component_falls_back_to_the_defaults() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let game_object = spawn_volumes(&mut scene, &[volume(true, 0, 2.0)])[0];
    assert_eq!(
        PostProcessSettings::resolve(&scene).tonemapping.exposure_ev,
        2.0
    );
    assert!(scene.set_component_enabled::<ComponentTonemap>(game_object, false));
    assert_eq!(
        PostProcessSettings::resolve(&scene).tonemapping,
        ComponentTonemap::default()
    );
}

/// The names of the passes `passes` plans for `scene`'s settings.
fn planned(passes: &[Box<dyn PostProcessPass>], scene: &Scene) -> Vec<String> {
    ChainPlan::build(passes, &PostProcessSettings::resolve(scene))
        .passes
        .iter()
        .map(|planned| passes[planned.pass].name().to_string())
        .collect()
}

/// Adds effect `T` to `volume`, checks that it runs `pass` after tone
/// mapping or before it, that disabling the component switches the pass off
/// and enabling it back on again, and removes it.
fn check_effect<T: Component + Default + Send + Sync + 'static>(
    passes: &[Box<dyn PostProcessPass>],
    scene: &mut Scene,
    volume: GameObject,
    pass: &str,
) {
    assert_eq!(planned(passes, scene), ["tonemap"], "{pass}: before");
    scene.add_component(volume, T::default());
    let mut names = planned(passes, scene);
    names.sort();
    let mut expected = vec![pass.to_string(), "tonemap".to_string()];
    expected.sort();
    assert_eq!(
        names, expected,
        "{pass}: adding the component runs its pass"
    );

    assert!(scene.set_component_enabled::<T>(volume, false));
    assert_eq!(
        planned(passes, scene),
        ["tonemap"],
        "{pass}: disabling the component switches its pass off"
    );
    assert!(scene.set_component_enabled::<T>(volume, true));
    assert_eq!(planned(passes, scene).len(), 2, "{pass}: enabled again");
    scene.remove_component::<T>(volume);
    assert_eq!(planned(passes, scene), ["tonemap"], "{pass}: removed");
}

#[test]
fn adding_or_disabling_an_effect_component_switches_its_pass_in_the_plan() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let renderer = SceneRenderer::new(&context, Default::default(), (SIZE, SIZE))
        .expect("scene renderer should load its assets");
    let passes = renderer.post_process().passes();

    let mut scene = context.registries.scene();
    let volume = scene.create(None, None);
    scene.add_component(volume, ComponentGlobalPostProcessVolume::default());
    check_effect::<ComponentAmbientOcclusion>(passes, &mut scene, volume, "ssao");
    check_effect::<ComponentDepthOfField>(passes, &mut scene, volume, "depth_of_field");
    check_effect::<ComponentBloom>(passes, &mut scene, volume, "bloom");
    check_effect::<ComponentColorGrading>(passes, &mut scene, volume, "color_grading");
    check_effect::<ComponentVignette>(passes, &mut scene, volume, "color_grading");
    check_effect::<ComponentAntiAliasing>(passes, &mut scene, volume, "fxaa");

    // A disabled volume's effects are off too.
    scene.add_component(volume, ComponentBloom::default());
    assert_eq!(planned(passes, &scene), ["bloom", "tonemap"]);
    scene.set_component_enabled::<ComponentGlobalPostProcessVolume>(volume, false);
    assert_eq!(planned(passes, &scene), ["tonemap"]);
}

/// Renders an empty scene cleared to `clear` through a full
/// [`SceneRenderer`] frame, and returns the renderer.
fn render_cleared_scene(
    context: &crate::context::ReadOnlyAssetContext,
    clear: Color32,
    volumes: &[VolumeSpec],
) -> SceneRenderer {
    let mut builder = SceneBuilder::new(context.registries.clone());
    spawn_volumes(builder.scene_mut(), volumes);
    let scene = builder.finish();
    let mut renderer = SceneRenderer::new(
        context,
        SceneRendererOptions {
            samples: 1,
            clear_color: clear,
            ..Default::default()
        },
        (SIZE, SIZE),
    )
    .expect("scene renderer should load its assets");
    let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 100.0);
    renderer.render_scene_base(&camera, &Default::default(), &scene, None, None);
    renderer.finalize_scene();
    renderer
}

fn read_scene_readback(renderer: &SceneRenderer, render_context: &RenderContext) -> [u8; 4] {
    let mut readback = renderer
        .submit_scene_texture_readback()
        .expect("scene texture has a size");
    loop {
        let _ = render_context.device().poll(wgpu::Maintain::Wait);
        match readback.try_finish(render_context) {
            ReadbackPoll::Pending => continue,
            ReadbackPoll::Ready(image) => break image.get_pixel(0, 0).0,
            ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
        }
    }
}

#[test]
fn tonemap_output_is_display_linear() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    // White at +2 EV is linear HDR 4.0. Alpha 64 / 255.
    let renderer = render_cleared_scene(
        &context,
        Color32::from_rgba_premultiplied(255, 255, 255, 64),
        &[volume(true, 0, 2.0)],
    );

    // The scene texture holds the curve's linear output, with no gamma...
    let texel = read_rgba16f(&render_context, renderer.scene_texture());
    let expected = aces_reference([4.0; 3]);
    for channel in 0..3 {
        assert_close(texel[channel], expected[channel], "scene texture");
    }
    assert_close(texel[3], 64.0 / 255.0, "scene texture alpha");
    // ...which egui and the readback both sRGB-encode for display.
    let pixel = read_scene_readback(&renderer, &render_context);
    let encoded = linear_to_srgb(expected[0]);
    for channel in 0..3 {
        assert!(
            pixel[channel].abs_diff(encoded) <= 1,
            "readback should sRGB-encode the display-linear value {}: expected {encoded}, got {}",
            expected[0],
            pixel[channel]
        );
    }
    assert_eq!(pixel[3], 64, "readback keeps alpha linear");
    assert_eq!(
        renderer.post_process().pool().allocated(),
        0,
        "a tone-map-only frame allocates no intermediate target"
    );
    assert_eq!(renderer.last_post_process_plan().passes.len(), 1);
}

#[test]
fn scenes_without_an_enabled_volume_are_still_tone_mapped() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let expected = aces_reference([1.0; 3])[0];

    for volumes in [vec![], vec![volume(false, 0, 5.0)]] {
        let renderer = render_cleared_scene(&context, Color32::WHITE, &volumes);
        assert_eq!(
            *renderer.post_process_settings(),
            PostProcessSettings::default()
        );
        let texel = read_rgba16f(&render_context, renderer.scene_texture());
        assert_close(texel[0], expected, "white through the default ACES curve");
    }

    let renderer = render_cleared_scene(
        &context,
        Color32::WHITE,
        &[volume(true, 0, 1.0), volume(true, 4, -1.0)],
    );
    assert_eq!(
        renderer.post_process_settings().tonemapping.exposure_ev,
        -1.0,
        "the highest-priority volume drives the frame"
    );
    let texel = read_rgba16f(&render_context, renderer.scene_texture());
    assert_close(texel[0], aces_reference([0.5; 3])[0], "white at -1 EV");
}

// FXAA.

const EDGE_SIZE: u32 = 16;

/// `value` as half-float bits. Exact for the normal values these tests use.
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

fn image_target(render_context: &Arc<RenderContext>, label: &'static str) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: EDGE_SIZE,
                height: EDGE_SIZE,
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

/// An `EDGE_SIZE` square image, row-major, one texel per pixel.
type Image = Vec<[f32; 4]>;

fn upload_image(render_context: &RenderContext, texture: &Texture, image: &Image) {
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
            bytes_per_row: Some(EDGE_SIZE * 8),
            rows_per_image: Some(EDGE_SIZE),
        },
        texture.descriptor.size,
    );
}

fn read_image(render_context: &RenderContext, texture: &Texture) -> Image {
    let device = render_context.device();
    let row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fxaa_test_readback"),
        size: (row * EDGE_SIZE) as u64,
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
                rows_per_image: Some(EDGE_SIZE),
            },
        },
        texture.descriptor.size,
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let image = (0..EDGE_SIZE as usize)
        .flat_map(|y| (0..EDGE_SIZE as usize).map(move |x| (x, y)))
        .map(|(x, y)| {
            let texel = y * row as usize + x * 8;
            std::array::from_fn(|channel| {
                let at = texel + channel * 2;
                half_to_f32(u16::from_le_bytes([data[at], data[at + 1]]))
            })
        })
        .collect();
    drop(data);
    buffer.unmap();
    image
}

fn fxaa_on() -> PostProcessSettings {
    PostProcessSettings {
        anti_aliasing: Some(ComponentAntiAliasing::default()),
        ..Default::default()
    }
}

/// Runs a chain holding only the FXAA pass over `image`.
fn run_fxaa(context: &crate::context::ReadOnlyAssetContext, image: &Image) -> Image {
    let render_context = context.render_context.clone();
    let mut chain = PostProcessChain::new(context).expect("chain shaders load");
    chain.push(Box::new(FxaaPass::new(context).expect("fxaa loads")));
    let source = image_target(&render_context, "fxaa_test_source");
    let output = image_target(&render_context, "fxaa_test_output");
    let depth = depth_target(&render_context, 1);
    upload_image(&render_context, &source, image);
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    let plan = chain.run(
        &render_context,
        &mut encoder,
        &fxaa_on(),
        ChainTargets {
            source: &source,
            output: &output,
            depth: &depth,
            particle_depth: None,
        },
    );
    render_context.queue().submit(Some(encoder.finish()));
    assert_eq!(plan.passes.len(), 1, "the FXAA pass runs");
    read_image(&render_context, &output)
}

#[test]
fn fxaa_is_off_by_default_and_a_partial_component_loads_with_defaults() {
    assert_eq!(PostProcessSettings::default().anti_aliasing, None);
    let defaults = ComponentAntiAliasing::default();
    assert_eq!(defaults.edge_threshold, 0.166);
    assert_eq!(defaults.subpixel, 0.75);

    let partial = r#"{"edge_threshold":0.1}"#;
    let settings: ComponentAntiAliasing = serde_json::from_str(partial).expect("partial loads");
    assert_eq!(settings.edge_threshold, 0.1);
    assert_eq!(settings.subpixel, 0.75);
}

#[test]
fn with_fxaa_disabled_the_plan_is_unchanged() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
    chain.push(Box::new(FxaaPass::new(&context).expect("fxaa loads")));
    chain.push(Box::new(TonemapPass::new(&context).expect("tonemap loads")));
    let names = |plan: &ChainPlan| {
        plan.passes
            .iter()
            .map(|planned| chain.passes()[planned.pass].name().to_string())
            .collect::<Vec<_>>()
    };

    let plan = ChainPlan::build(chain.passes(), &Default::default());
    assert_eq!(names(&plan), ["tonemap"]);
    assert_eq!(
        targets(&plan),
        vec![(PassTarget::Source, PassTarget::Output)]
    );
    assert_eq!(plan.intermediates, 0);

    let plan = ChainPlan::build(chain.passes(), &fxaa_on());
    assert_eq!(
        names(&plan),
        ["tonemap", "fxaa"],
        "FXAA runs after tone mapping"
    );
    assert_eq!(plan.intermediates, 1);
}

#[test]
fn fxaa_smooths_a_hard_diagonal_edge_and_keeps_alpha() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // White above the diagonal, black below. Alpha differs on each side, so
    // a blended alpha would show.
    let size = EDGE_SIZE as usize;
    let image: Image = (0..size * size)
        .map(|index| {
            let (x, y) = (index % size, index / size);
            if x > y {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.0, 0.0, 0.0, 0.5]
            }
        })
        .collect();

    let output = run_fxaa(&context, &image);

    let blended = output
        .iter()
        .filter(|texel| texel[0] > 0.05 && texel[0] < 0.95)
        .count();
    assert!(
        blended >= size,
        "the staircase gains intermediate values along the edge, got {blended}"
    );
    for (index, (out, input)) in output.iter().zip(&image).enumerate() {
        let (x, y) = (index % size, index / size);
        assert_eq!(out[3], input[3], "alpha at ({x}, {y}) passes through");
        if x.abs_diff(y) > 2 {
            assert_eq!(out[0], input[0], "({x}, {y}) is away from the edge");
        }
    }
}

#[test]
fn fxaa_leaves_a_flat_colour_and_its_alpha_unchanged() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let flat = [0.25, 0.5, 0.75, 0.5];
    let image: Image = vec![flat; (EDGE_SIZE * EDGE_SIZE) as usize];

    let output = run_fxaa(&context, &image);

    for texel in output {
        assert_eq!(texel, flat);
    }
}
