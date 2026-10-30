use super::*;
use crate::assets::texture::Texture;
use crate::render::scene_renderer::half_to_f32;
use crate::render::scene_renderer::CameraUniform;
use crate::render::{Camera, GridRenderer, RenderContext, RenderUtils, SceneRendererOptions};
use crate::test_support::{test_adapter, TestAssets};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use nalgebra_glm as glm;
use std::sync::{Arc, Mutex};

const WIDTH: u32 = 4;
const HEIGHT: u32 = 1;

/// Draws one constant color over every pixel inside its viewport.
const SOLID_SHADER: &str = "
@group(0) @binding(0) var<uniform> color: vec4f;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.5, 1.0);
}
@fragment fn fs_main() -> @location(0) vec4f {
    return color;
}
";

/// An overlay that blends `color` into the overlay target the way the grid,
/// gizmos and outline do, over the pixels right of `left`, which may fall
/// inside a pixel.
struct SolidOverlay {
    name: &'static str,
    enabled: bool,
    left: f32,
    color: [f32; 4],
    pipeline: Option<wgpu::RenderPipeline>,
    uniform: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    recorded: Arc<Mutex<Vec<&'static str>>>,
}

impl SolidOverlay {
    fn new(name: &'static str, recorded: &Arc<Mutex<Vec<&'static str>>>) -> Self {
        Self {
            name,
            enabled: true,
            left: 0.0,
            color: [1.0; 4],
            pipeline: None,
            uniform: None,
            recorded: recorded.clone(),
        }
    }
}

impl OverlayPass for SolidOverlay {
    fn name(&self) -> &str {
        self.name
    }

    fn enabled(&self, _context: &OverlayContext) -> bool {
        self.enabled
    }

    fn prepare(&mut self, context: &OverlayContext) {
        let device = context.render_context.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solid_overlay"),
            source: wgpu::ShaderSource::Wgsl(SOLID_SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("solid_overlay"),
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
                targets: &[Some(RenderUtils::color_alpha_blending(context.format))],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: false,
                ..RenderUtils::depth_default(wgpu::TextureFormat::Depth32Float)
            }),
            multisample: RenderUtils::multisample_default(context.samples),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("solid_overlay"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        context
            .render_context
            .queue()
            .write_buffer(&buffer, 0, bytemuck::cast_slice(&self.color));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("solid_overlay"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        self.pipeline = Some(pipeline);
        self.uniform = Some((buffer, bind_group));
    }

    fn record(&mut self, _context: &OverlayContext, render_pass: &mut wgpu::RenderPass<'_>) {
        self.recorded.lock().unwrap().push(self.name);
        render_pass.set_viewport(
            self.left,
            0.0,
            WIDTH as f32 - self.left,
            HEIGHT as f32,
            0.0,
            1.0,
        );
        render_pass.set_pipeline(self.pipeline.as_ref().expect("prepared"));
        render_pass.set_bind_group(0, &self.uniform.as_ref().expect("prepared").1, &[]);
        RenderUtils::draw_fullscreen(render_pass);
    }
}

fn texture(
    render_context: &Arc<RenderContext>,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Texture {
    sized_texture(render_context, format, samples, WIDTH, HEIGHT)
}

fn sized_texture(
    render_context: &Arc<RenderContext>,
    format: wgpu::TextureFormat,
    samples: u32,
    width: u32,
    height: u32,
) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("overlay_test_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | if samples == 1 {
                    wgpu::TextureUsages::COPY_SRC
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        },
        None,
        None,
        false,
    )
}

/// Clears `color` to `clear` and `depth` to the far plane.
fn clear(encoder: &mut wgpu::CommandEncoder, color: &Texture, clear: f64, depth: &Texture) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("overlay_test_clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &color.view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: clear,
                    g: clear,
                    b: clear,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("overlay_test_clear_depth"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
            &depth.view,
            1.0,
            None,
        )),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

/// Every texel of the one row of an `Rgba16Float` texture.
fn read_row(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 4]> {
    read_texels(render_context, texture)
}

/// Every texel of an `Rgba16Float` texture, row by row.
fn read_texels(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 4]> {
    let size = texture.descriptor.size;
    let row_bytes = size.width * 8;
    let padded_row_bytes = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let device = render_context.device();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay_test_readback"),
        size: (padded_row_bytes * size.height) as u64,
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
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes
        .chunks(padded_row_bytes as usize)
        .flat_map(|row| row[..row_bytes as usize].as_chunks::<8>().0.to_vec())
        .map(|texel| {
            std::array::from_fn(|channel| {
                half_to_f32(u16::from_le_bytes([
                    texel[channel * 2],
                    texel[channel * 2 + 1],
                ]))
            })
        })
        .collect()
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    // Rgba16Float keeps about three decimal digits.
    assert!(
        (actual - expected).abs() <= 2e-3,
        "{what}: expected {expected}, got {actual}"
    );
}

/// Runs `overlays` through `compositor` over an output cleared to
/// `background`, at `samples`, and returns the output row and how many
/// overlays recorded.
fn composite(
    render_context: &Arc<RenderContext>,
    compositor: &mut OverlayCompositor,
    overlays: &mut [&mut dyn OverlayPass],
    samples: u32,
    background: f64,
) -> (Vec<[f32; 4]>, usize) {
    let output = texture(render_context, wgpu::TextureFormat::Rgba16Float, 1);
    let depth = texture(render_context, wgpu::TextureFormat::Depth32Float, samples);
    let object_ids = texture(render_context, wgpu::TextureFormat::R32Uint, 1);
    let options = SceneRendererOptions::default();
    let context = OverlayContext {
        render_context,
        options: &options,
        format: OverlayCompositor::FORMAT,
        samples,
        object_ids: &object_ids,
    };
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    clear(&mut encoder, &output, background, &depth);
    let recorded = compositor.run(&mut encoder, overlays, &context, &depth, &output);
    render_context.queue().submit(Some(encoder.finish()));
    (read_row(render_context, &output), recorded)
}

#[test]
fn a_half_covered_overlay_edge_composites_without_darkening() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    const SAMPLES: u32 = 4;
    let flags = test_adapter()
        .get_texture_format_features(OverlayCompositor::FORMAT)
        .flags;
    if !flags.sample_count_supported(SAMPLES) {
        eprintln!("skipping: the test adapter cannot render {SAMPLES} samples");
        return;
    }
    let mut compositor = OverlayCompositor::new(&context).expect("composite shader loads");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    // The overlay's left edge splits pixel 1 down the middle: two of the
    // standard four sample positions lie right of it, at x = 1.625 and 1.875.
    let mut overlay = SolidOverlay::new("edge", &recorded);
    overlay.left = 1.5;
    overlay.color = [1.0, 1.0, 1.0, 0.8];
    let background = 0.25;

    let (row, _) = composite(
        &render_context,
        &mut compositor,
        &mut [&mut overlay],
        SAMPLES,
        background as f64,
    );

    // Alpha blending into the transparent target leaves premultiplied color:
    // a covered sample holds (0.8, 0.8), an uncovered one (0, 0), and the
    // resolve averages them to (0.4, 0.4) at the edge.
    let target = read_row(&render_context, compositor.target().expect("drawn"));
    assert_close(target[1][0], 0.4, "resolved edge color");
    assert_close(target[1][3], 0.4, "resolved edge coverage");

    // rgb + dst * (1 - a). Multiplying the color by alpha again would give
    // 0.4 * 0.4 + 0.25 * 0.6 = 0.31, a darkened edge.
    assert_close(row[0][0], background, "uncovered pixel");
    assert_close(row[1][0], 0.4 + background * 0.6, "half-covered edge pixel");
    assert_close(row[2][0], 0.8 + background * 0.2, "covered pixel");
    assert_close(row[3][0], 0.8 + background * 0.2, "covered pixel");
    assert_close(
        row[1][3],
        1.0,
        "the composite keeps an opaque output opaque",
    );
}

#[test]
fn enabled_overlays_record_in_order_and_the_target_exists_only_while_one_is() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let mut compositor = OverlayCompositor::new(&context).expect("composite shader loads");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut grid = SolidOverlay::new("grid", &recorded);
    let mut gizmos = SolidOverlay::new("gizmos", &recorded);
    gizmos.enabled = false;
    let mut outline = SolidOverlay::new("outline", &recorded);
    outline.color = [0.0, 1.0, 0.0, 1.0];

    let (row, count) = composite(
        &render_context,
        &mut compositor,
        &mut [&mut grid, &mut gizmos, &mut outline],
        1,
        0.0,
    );
    assert_eq!(count, 2);
    assert_eq!(*recorded.lock().unwrap(), ["grid", "outline"]);
    assert_eq!(
        row[0],
        [0.0, 1.0, 0.0, 1.0],
        "the last overlay draws on top"
    );
    let target = compositor.target().expect("an overlay is enabled");
    assert_eq!(target.descriptor.format, wgpu::TextureFormat::Rgba16Float);

    for overlay in [&mut grid, &mut outline] {
        overlay.enabled = false;
    }
    let (row, count) = composite(
        &render_context,
        &mut compositor,
        &mut [&mut grid, &mut gizmos, &mut outline],
        1,
        0.5,
    );
    assert_eq!(count, 0);
    assert!(
        compositor.target().is_none(),
        "no overlay is enabled, so no overlay target is kept"
    );
    assert_eq!(row[0], [0.5, 0.5, 0.5, 1.0], "the output is left as it was");
}

/// A camera above the grid looking down at the world origin, where both grid
/// layers and both axes overlap.
fn grid_camera_uniform(size: u32) -> CameraUniform {
    let camera = Camera::new(1.0, 70f32.to_radians(), 0.1, 1000.0);
    let view = glm::look_at_lh(
        &glm::vec3(0.5, 3.0, -3.0),
        &glm::vec3(0.0, 0.0, 0.0),
        &glm::vec3(0.0, 1.0, 0.0),
    );
    CameraUniform {
        projection: camera.projection.into(),
        view: view.into(),
        inverse_projection: glm::inverse(&camera.projection).into(),
        inverse_view: glm::inverse(&view).into(),
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        viewport_size: [size as f32; 2],
    }
}

#[test]
fn the_grid_leaves_coverage_alpha_in_range_and_color_no_brighter_than_it() {
    const SIZE: u32 = 64;
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let camera_buffer =
        render_context
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("grid_test_camera"),
                contents: bytemuck::cast_slice(&[grid_camera_uniform(SIZE)]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
    let mut grid = GridRenderer::new(&context, &camera_buffer).expect("grid shader loads");
    let mut compositor = OverlayCompositor::new(&context).expect("composite shader loads");

    let output = sized_texture(
        &render_context,
        wgpu::TextureFormat::Rgba16Float,
        1,
        SIZE,
        SIZE,
    );
    let depth = sized_texture(
        &render_context,
        wgpu::TextureFormat::Depth32Float,
        1,
        SIZE,
        SIZE,
    );
    let object_ids = sized_texture(&render_context, wgpu::TextureFormat::R32Uint, 1, 1, 1);
    let options = SceneRendererOptions {
        grid: true,
        ..Default::default()
    };
    let overlay_context = OverlayContext {
        render_context: &render_context,
        options: &options,
        format: OverlayCompositor::FORMAT,
        samples: 1,
        object_ids: &object_ids,
    };
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    clear(&mut encoder, &output, 0.5, &depth);
    let recorded = compositor.run(
        &mut encoder,
        &mut [&mut grid],
        &overlay_context,
        &depth,
        &output,
    );
    render_context.queue().submit(Some(encoder.finish()));
    assert_eq!(recorded, 1);

    let texels = read_texels(&render_context, compositor.target().expect("drawn"));
    // The origin is at the image centre: both layers and both axes cover it.
    let centre = texels[(SIZE / 2 * SIZE + SIZE / 2) as usize];
    assert!(
        centre[3] > 0.5,
        "the grid covers the origin, where its layers overlap: {centre:?}"
    );
    // Rgba16Float rounds to about three decimal digits.
    const EPSILON: f32 = 2e-3;
    for (index, texel) in texels.iter().enumerate() {
        let (x, y) = (index as u32 % SIZE, index as u32 / SIZE);
        let alpha = texel[3];
        assert!(
            (-EPSILON..=1.0 + EPSILON).contains(&alpha),
            "texel ({x}, {y}) has alpha {alpha} outside [0, 1]: {texel:?}"
        );
        for channel in &texel[..3] {
            assert!(
                *channel >= -EPSILON && *channel <= alpha + EPSILON,
                "texel ({x}, {y}) is brighter than its coverage: {texel:?}"
            );
        }
    }
}

#[test]
fn an_overlay_texel_with_coverage_above_one_never_subtracts_the_scene() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let mut compositor = OverlayCompositor::new(&context).expect("composite shader loads");
    let recorded = Arc::new(Mutex::new(Vec::new()));
    // Alpha blending a = 2 into the transparent target leaves rgb 2 * 0.3
    // and alpha 2: outside the premultiplied contract.
    let mut overlay = SolidOverlay::new("over_covered", &recorded);
    overlay.color = [0.3, 0.3, 0.3, 2.0];

    for background in [0.25, 0.75] {
        let (row, _) = composite(
            &render_context,
            &mut compositor,
            &mut [&mut overlay],
            1,
            background,
        );
        let target = read_row(&render_context, compositor.target().expect("drawn"));
        assert!(target[0][3] > 1.0, "the target holds coverage above one");
        // Held to coverage one and color 0.6, the overlay hides the scene. A
        // factor of 1 - a = -1 on the scene would give 0.6 - background.
        assert_close(row[0][0], 0.6, "over-covered pixel");
    }
}
