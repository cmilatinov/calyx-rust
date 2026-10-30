use super::*;
use crate::assets::texture::Texture;
use crate::render::scene_renderer::half_to_f32;
use crate::render::{RenderContext, RenderUtils, SceneRendererOptions};
use crate::test_support::{test_adapter, TestAssets};
use egui_wgpu::wgpu;
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
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("overlay_test_texture"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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
    let device = render_context.device();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay_test_readback"),
        size: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64,
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
                bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                rows_per_image: Some(1),
            },
        },
        texture.descriptor.size,
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let bytes = slice.get_mapped_range().to_vec();
    buffer.unmap();
    bytes[..(WIDTH * 8) as usize]
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
