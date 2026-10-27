use super::*;
use crate::assets::texture::Texture;
use crate::component::ComponentGlobalPostProcessVolume;
use crate::render::scene_renderer::{half_to_f32, linear_to_srgb};
use crate::render::{Camera, ReadbackPoll, RenderContext, SceneRenderer, SceneRendererOptions};
use crate::test_support::{test_adapter, SceneBuilder, TestAssets};
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
        let mut pass = begin_fullscreen_pass(encoder, &io.output.view, self.name);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
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
        tonemapping: TonemapSettings {
            exposure_ev: 2.0,
            ..Default::default()
        },
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

fn volume(enabled: bool, priority: i32, exposure_ev: f32) -> ComponentGlobalPostProcessVolume {
    ComponentGlobalPostProcessVolume {
        enabled,
        priority,
        settings: PostProcessSettings {
            tonemapping: TonemapSettings {
                exposure_ev,
                ..Default::default()
            },
        },
    }
}

#[test]
fn the_enabled_volume_with_the_highest_priority_is_selected() {
    let volumes = [
        volume(true, 1, 1.0),
        volume(false, 9, 9.0),
        volume(true, 3, 3.0),
        volume(true, 2, 2.0),
    ];
    let selection = select_volume(&volumes);
    assert_eq!(selection.settings.tonemapping.exposure_ev, 3.0);
    assert_eq!(selection.tie, None);
}

#[test]
fn a_priority_tie_selects_the_first_found_and_reports_it() {
    let volumes = [
        volume(true, 2, 1.0),
        volume(true, 2, 2.0),
        volume(true, 0, 3.0),
    ];
    let selection = select_volume(&volumes);
    assert_eq!(selection.settings.tonemapping.exposure_ev, 1.0);
    assert_eq!(selection.tie, Some(2));

    // A higher priority after a tie wins outright.
    let volumes = [
        volume(true, 2, 1.0),
        volume(true, 2, 2.0),
        volume(true, 5, 3.0),
    ];
    let selection = select_volume(&volumes);
    assert_eq!(selection.settings.tonemapping.exposure_ev, 3.0);
    assert_eq!(selection.tie, None);
}

#[test]
fn no_enabled_volume_selects_the_default_settings() {
    assert_eq!(
        select_volume(std::iter::empty()).settings,
        PostProcessSettings::default()
    );
    let selection = select_volume(&[volume(false, 5, 4.0)]);
    assert_eq!(selection.settings, PostProcessSettings::default());
    assert_eq!(selection.tie, None);
    let default = PostProcessSettings::default().tonemapping;
    assert_eq!(default.preset, TonemappingPreset::Aces);
    assert_eq!(default.exposure_ev, 0.0);
}

/// Renders an empty scene cleared to `clear` through a full
/// [`SceneRenderer`] frame, and returns the renderer.
fn render_cleared_scene(
    context: &crate::context::ReadOnlyAssetContext,
    clear: Color32,
    volumes: &[ComponentGlobalPostProcessVolume],
) -> SceneRenderer {
    let mut builder = SceneBuilder::new(context.registries.clone());
    for (index, volume) in volumes.iter().enumerate() {
        let game_object = builder.spawn(format!("volume_{index}"));
        builder.add_component(game_object, volume.clone());
    }
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
