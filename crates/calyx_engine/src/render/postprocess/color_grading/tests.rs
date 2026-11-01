use super::*;
use crate::assets::texture::Texture;
use crate::component::{ComponentAntiAliasing, ComponentColorGrading, ComponentVignette};
use crate::render::postprocess::{ChainPlan, ChainTargets, PostProcessChain, TonemapPass};
use crate::render::scene_renderer::half_to_f32;
use crate::render::{RenderContext, SceneRenderer};
use crate::test_support::TestAssets;
use egui::Color32;
use std::sync::Arc;

// Odd sizes put a pixel centre exactly on the screen centre, and a wide image
// checks the vignette's aspect correction.
const WIDTH: u32 = 15;
const HEIGHT: u32 = 9;
const CENTRE: (usize, usize) = (7, 4);
/// Half floats hold about three decimal digits.
const TOLERANCE: f32 = 2e-3;

/// A `WIDTH` by `HEIGHT` image, row-major, one RGBA texel per pixel.
type Image = Vec<[f32; 4]>;

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

fn target(render_context: &Arc<RenderContext>, format: wgpu::TextureFormat) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("color_grading_test_target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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
            bytes_per_row: Some(WIDTH * 8),
            rows_per_image: Some(HEIGHT),
        },
        texture.descriptor.size,
    );
}

fn read(render_context: &RenderContext, texture: &Texture) -> Image {
    let device = render_context.device();
    let row = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("color_grading_test_readback"),
        size: (row * HEIGHT) as u64,
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
                rows_per_image: Some(HEIGHT),
            },
        },
        texture.descriptor.size,
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let image = (0..HEIGHT as usize)
        .flat_map(|y| (0..WIDTH as usize).map(move |x| (x, y)))
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

/// A colourful image with a different alpha in every pixel.
fn test_image() -> Image {
    (0..(WIDTH * HEIGHT) as usize)
        .map(|index| {
            let (x, y) = (index % WIDTH as usize, index / WIDTH as usize);
            let u = x as f32 / (WIDTH - 1) as f32;
            let v = y as f32 / (HEIGHT - 1) as f32;
            [
                0.05 + 0.9 * u,
                0.1 + 0.8 * v,
                0.9 - 0.7 * u * v,
                0.25 + 0.75 * (index as f32 / (WIDTH * HEIGHT) as f32),
            ]
        })
        .collect()
}

fn uniform_image(texel: [f32; 4]) -> Image {
    vec![texel; (WIDTH * HEIGHT) as usize]
}

fn at(image: &Image, (x, y): (usize, usize)) -> [f32; 4] {
    image[y * WIDTH as usize + x]
}

/// Runs a chain holding only the colour grading pass over `image`.
fn run_grading(settings: &PostProcessSettings, image: &Image) -> Image {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let render_context = context.render_context.clone();
    let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
    chain.push(Box::new(
        ColorGradingPass::new(&context).expect("color grading loads"),
    ));
    let source = target(&render_context, wgpu::TextureFormat::Rgba16Float);
    let output = target(&render_context, wgpu::TextureFormat::Rgba16Float);
    let depth = target(&render_context, wgpu::TextureFormat::Depth32Float);
    upload(&render_context, &source, image);
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    let plan = chain.run(
        &render_context,
        &mut encoder,
        settings,
        ChainTargets {
            source: &source,
            output: &output,
            depth: &depth,
            particle_depth: None,
        },
    );
    render_context.queue().submit(Some(encoder.finish()));
    assert_eq!(plan.passes.len(), 1, "the colour grading pass runs");
    read(&render_context, &output)
}

fn grading(grading: ComponentColorGrading) -> PostProcessSettings {
    PostProcessSettings {
        color_grading: Some(grading),
        ..Default::default()
    }
}

fn vignette(vignette: ComponentVignette) -> PostProcessSettings {
    PostProcessSettings {
        vignette: Some(vignette),
        ..Default::default()
    }
}

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn both_effects_are_off_by_default_and_partial_components_load_with_defaults() {
    let defaults = PostProcessSettings::default();
    assert_eq!(defaults.color_grading, None, "grading is off by default");
    assert_eq!(defaults.vignette, None, "the vignette is off by default");

    let grading: ComponentColorGrading =
        serde_json::from_str(r#"{"saturation":0.5}"#).expect("partial grading loads");
    assert_eq!(grading.saturation, 0.5);
    assert_eq!(grading.contrast, 1.0);
    let vignette: ComponentVignette =
        serde_json::from_str(r#"{"intensity":0.7}"#).expect("partial vignette loads");
    assert_eq!(vignette.intensity, 0.7);
    assert_eq!(vignette.smoothness, ComponentVignette::default().smoothness);
}

#[test]
fn with_grading_and_vignette_disabled_the_pass_is_skipped() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut chain = PostProcessChain::new(&context).expect("chain shaders load");
    chain.push(Box::new(
        ColorGradingPass::new(&context).expect("color grading loads"),
    ));
    chain.push(Box::new(TonemapPass::new(&context).expect("tonemap loads")));
    let names = |plan: &ChainPlan| {
        plan.passes
            .iter()
            .map(|planned| chain.passes()[planned.pass].name().to_string())
            .collect::<Vec<_>>()
    };

    let plan = ChainPlan::build(chain.passes(), &Default::default());
    assert_eq!(names(&plan), ["tonemap"]);
    assert_eq!(plan.intermediates, 0);

    for settings in [grading(Default::default()), vignette(Default::default())] {
        let plan = ChainPlan::build(chain.passes(), &settings);
        assert_eq!(
            names(&plan),
            ["tonemap", "color_grading"],
            "either effect runs the pass, after tone mapping"
        );
    }
}

#[test]
fn the_scene_renderer_grades_before_fxaa() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let renderer = SceneRenderer::new(&context, Default::default(), (WIDTH, HEIGHT))
        .expect("scene renderer should load its assets");
    let settings = PostProcessSettings {
        anti_aliasing: Some(ComponentAntiAliasing::default()),
        ..grading(Default::default())
    };
    let chain = renderer.post_process();

    let plan = ChainPlan::build(chain.passes(), &settings);

    let names: Vec<_> = plan
        .passes
        .iter()
        .map(|planned| chain.passes()[planned.pass].name())
        .collect();
    assert_eq!(names, ["tonemap", "color_grading", "fxaa"]);
}

#[test]
fn neutral_settings_return_the_input() {
    let image = test_image();
    let settings = PostProcessSettings {
        vignette: Some(ComponentVignette {
            intensity: 0.0,
            ..Default::default()
        }),
        ..grading(Default::default())
    };

    let output = run_grading(&settings, &image);

    for (index, (out, input)) in output.iter().zip(&image).enumerate() {
        for channel in 0..4 {
            assert_close(out[channel], input[channel], &format!("texel {index}"));
        }
    }
}

#[test]
fn zero_saturation_gives_grey_at_the_input_luminance() {
    let output = run_grading(
        &grading(ComponentColorGrading {
            saturation: 0.0,
            ..Default::default()
        }),
        &uniform_image([0.8, 0.4, 0.1, 1.0]),
    );

    // Rec. 709 luminance of the linear input.
    let luma = 0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.1;
    for texel in output {
        for value in &texel[..3] {
            assert_close(*value, luma, "grey");
        }
    }
}

#[test]
fn contrast_keeps_mid_grey_and_spreads_the_rest() {
    let settings = grading(ComponentColorGrading {
        contrast: 1.5,
        ..Default::default()
    });
    let mid = run_grading(&settings, &uniform_image([0.18, 0.18, 0.18, 1.0]));
    assert_close(at(&mid, CENTRE)[0], 0.18, "mid grey is the pivot");

    let dark = run_grading(&settings, &uniform_image([0.05, 0.05, 0.05, 1.0]));
    let bright = run_grading(&settings, &uniform_image([0.4, 0.4, 0.4, 1.0]));
    // 0.18 * (x / 0.18)^1.5, worked by hand.
    assert_close(at(&dark, CENTRE)[0], 0.026352, "darks get darker");
    assert_close(at(&bright, CENTRE)[0], 0.596290, "brights get brighter");
}

#[test]
fn positive_temperature_warms_and_positive_tint_adds_magenta() {
    let grey = uniform_image([0.5, 0.5, 0.5, 1.0]);

    let warm = at(
        &run_grading(
            &grading(ComponentColorGrading {
                temperature: 0.5,
                ..Default::default()
            }),
            &grey,
        ),
        CENTRE,
    );
    assert!(warm[0] > warm[2] + 0.02, "warmer is redder: {warm:?}");

    let magenta = at(
        &run_grading(
            &grading(ComponentColorGrading {
                tint: 0.5,
                ..Default::default()
            }),
            &grey,
        ),
        CENTRE,
    );
    assert!(
        magenta[1] + 0.02 < magenta[0] && magenta[1] + 0.02 < magenta[2],
        "magenta has less green: {magenta:?}"
    );
}

#[test]
fn the_vignette_darkens_the_corners_and_leaves_the_centre_unchanged() {
    let image = uniform_image([0.8, 0.6, 0.4, 1.0]);
    let output = run_grading(
        &vignette(ComponentVignette {
            intensity: 0.5,
            ..Default::default()
        }),
        &image,
    );

    let centre = at(&output, CENTRE);
    for channel in 0..3 {
        assert_close(centre[channel], image[0][channel], "the centre");
    }
    let (right, bottom) = (WIDTH as usize - 1, HEIGHT as usize - 1);
    for corner in [(0, 0), (right, 0), (0, bottom), (right, bottom)] {
        let texel = at(&output, corner);
        assert!(
            texel[0] < 0.5 * image[0][0],
            "the corner {corner:?} is darker: {texel:?}"
        );
    }
    // Aspect correction makes it round: the side edges, further from the
    // centre in pixels, darken more than the top and bottom.
    let side = at(&output, (0, CENTRE.1))[0];
    let top = at(&output, (CENTRE.0, 0))[0];
    assert!(side < top, "side {side} is darker than top {top}");
}

#[test]
fn alpha_is_preserved() {
    let image = test_image();
    let settings = PostProcessSettings {
        color_grading: Some(ComponentColorGrading {
            contrast: 1.4,
            saturation: 0.3,
            temperature: 0.6,
            tint: -0.4,
        }),
        vignette: Some(ComponentVignette {
            intensity: 0.8,
            smoothness: 0.5,
            color: Color32::from_rgb(40, 0, 80),
        }),
        ..Default::default()
    };

    let output = run_grading(&settings, &image);

    for (index, (out, input)) in output.iter().zip(&image).enumerate() {
        // Exactly the uploaded half-float alpha.
        let uploaded = half_to_f32(f32_to_half(input[3]));
        assert_eq!(out[3], uploaded, "alpha of texel {index}");
    }
}

#[test]
fn the_white_balance_is_the_identity_at_zero() {
    assert_eq!(
        white_balance(0.0, 0.0),
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    );
    // Near zero it is still close to the identity: no jump at 0.
    let near = white_balance(1e-4, 0.0);
    for (row, values) in near.iter().enumerate() {
        for (column, value) in values.iter().enumerate() {
            let identity = if row == column { 1.0 } else { 0.0 };
            assert!(
                (value - identity).abs() < 1e-3,
                "[{row}][{column}] = {value}"
            );
        }
    }
}
