//! Distance fog: what the pass writes for a known depth, and what a rendered
//! scene looks like through it.

use super::*;
use crate::assets::material::{Material, ShaderVariableValue};
use crate::assets::mesh::Mesh;
use crate::assets::texture::Texture;
use crate::assets::AssetRef;
use crate::component::{ComponentFog, ComponentGlobalPostProcessVolume, ComponentMesh, FogMode};
use crate::math::Transform;
use crate::render::postprocess::{CameraPlanes, ChainTargets, PostProcessChain};
use crate::render::scene_renderer::half_to_f32;
use crate::render::{Camera, ReadbackPoll, RenderContext, SceneRenderer, SceneRendererOptions};
use crate::test_support::{test_adapter, SceneBuilder, TestAssets};
use egui::{Color32, Rgba};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{self as glm, vec3, vec4, Vec3};
use std::sync::Arc;

const SIZE: (u32, u32) = (32, 32);
const BYTES_PER_ROW: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
/// Linear HDR colour the chain's source is cleared to, with partial alpha.
const COLOR: [f32; 4] = [2.0, 0.5, 0.25, 0.75];
/// Depth the skybox writes.
const SKYBOX_DEPTH: f32 = 0.9999999;
/// Half floats hold about three decimal digits.
const TOLERANCE: f32 = 4e-3;

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

fn fog(mode: FogMode) -> ComponentFog {
    ComponentFog {
        mode,
        color: Color32::from_rgb(64, 128, 255),
        intensity: 2.0,
        start: 4.0,
        end: 40.0,
        density: 0.05,
        opacity: 1.0,
    }
}

/// The linear HDR colour `fog` mixes towards.
fn fog_color(fog: &ComponentFog) -> [f32; 3] {
    let color = Rgba::from(fog.color);
    [color.r(), color.g(), color.b()].map(|channel| channel * fog.intensity)
}

fn texture(render_context: &Arc<RenderContext>, format: wgpu::TextureFormat) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("fog_test_texture"),
            size: wgpu::Extent3d {
                width: SIZE.0,
                height: SIZE.1,
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

/// Every texel of an `Rgba16Float` texture, row by row.
fn read_all(render_context: &RenderContext, texture: &Texture) -> Vec<[f32; 4]> {
    let device = render_context.device();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fog_test_readback"),
        size: (BYTES_PER_ROW * SIZE.1) as u64,
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
                rows_per_image: Some(SIZE.1),
            },
        },
        texture.descriptor.size,
    );
    render_context.queue().submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map failed"));
    let _ = device.poll(wgpu::Maintain::Wait);
    let bytes = slice.get_mapped_range();
    let mut texels = Vec::new();
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
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

/// Runs a chain of the fog pass alone over a source cleared to [`COLOR`],
/// with the whole scene depth cleared to `depth`, and returns the output.
fn run_fog(context: &ReadOnlyAssetContext, fog: ComponentFog, depth: f32) -> Vec<[f32; 4]> {
    let render_context = context.render_context.clone();
    let mut chain = PostProcessChain::new(context).expect("chain shaders load");
    chain.push(Box::new(FogPass::new(context).expect("fog loads")));
    chain.set_camera(planes());
    let source = texture(&render_context, wgpu::TextureFormat::Rgba16Float);
    let output = texture(&render_context, wgpu::TextureFormat::Rgba16Float);
    let scene_depth = texture(&render_context, wgpu::TextureFormat::Depth32Float);
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("fog_test_clear"),
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
        depth_stencil_attachment: Some(RenderUtils::depth_stencil_attachment(
            &scene_depth.view,
            depth,
            None,
        )),
        timestamp_writes: None,
        occlusion_query_set: None,
    }));
    let plan = chain.run(
        &render_context,
        &mut encoder,
        &PostProcessSettings {
            fog: Some(fog),
            ..Default::default()
        },
        ChainTargets {
            source: &source,
            output: &output,
            depth: &scene_depth,
            particle_depth: None,
            normals: None,
            albedo: None,
        },
    );
    assert_eq!(plan.passes.len(), 1, "fog runs");
    assert!(plan.depth, "fog reads the scene depth");
    render_context.queue().submit(Some(encoder.finish()));
    read_all(&render_context, &output)
}

/// Distance from the camera to a plane facing it at view depth `z`, along
/// the ray through the centre of pixel `(x, y)`.
fn distance_along_ray(x: u32, y: u32, z: f32) -> f32 {
    let projection = camera().projection;
    let ndc_x = (x as f32 + 0.5) / SIZE.0 as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - (y as f32 + 0.5) / SIZE.1 as f32 * 2.0;
    let slope_x = ndc_x / projection[(0, 0)];
    let slope_y = ndc_y / projection[(1, 1)];
    z * (1.0 + slope_x * slope_x + slope_y * slope_y).sqrt()
}

fn assert_texel(actual: [f32; 4], expected: [f32; 4], what: &str) {
    for channel in 0..4 {
        let tolerance = TOLERANCE * expected[channel].abs().max(1.0);
        assert!(
            (actual[channel] - expected[channel]).abs() <= tolerance,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }
}

#[test]
fn a_surface_fades_to_the_fog_colour_by_its_distance_along_the_ray() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    const Z: f32 = 12.0;
    let depth = planes().depth_of(Z);
    for mode in [FogMode::Linear, FogMode::Exponential] {
        let fog = ComponentFog {
            opacity: 0.8,
            ..fog(mode)
        };
        let color = fog_color(&fog);

        let output = run_fog(&context, fog, depth);

        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                let distance = distance_along_ray(x, y, Z);
                let thickness = match mode {
                    FogMode::Linear => (distance - 4.0) / 36.0,
                    FogMode::Exponential => 1.0 - (-0.05 * distance).exp(),
                };
                let amount = 0.8 * thickness;
                let expected = [
                    COLOR[0] + (color[0] - COLOR[0]) * amount,
                    COLOR[1] + (color[1] - COLOR[1]) * amount,
                    COLOR[2] + (color[2] - COLOR[2]) * amount,
                    COLOR[3],
                ];
                assert_texel(
                    output[(y * SIZE.0 + x) as usize],
                    expected,
                    &format!("{mode:?} fog at ({x}, {y}), {distance} away"),
                );
            }
        }
    }
}

#[test]
fn a_surface_before_linear_fog_starts_is_unchanged() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();

    let output = run_fog(&context, fog(FogMode::Linear), planes().depth_of(2.0));

    for texel in output {
        assert_texel(texel, COLOR, "a surface 2 units away, before 4");
    }
}

#[test]
fn the_background_takes_the_fog_colour() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // Linear fog ends at 40, well before the far plane at 100; the
    // background lies at the far plane, whether cleared or drawn by the
    // skybox.
    for depth in [1.0, SKYBOX_DEPTH] {
        let fog = fog(FogMode::Linear);
        let [r, g, b] = fog_color(&fog);

        let output = run_fog(&context, fog, depth);

        for (index, texel) in output.into_iter().enumerate() {
            assert_texel(
                texel,
                [r, g, b, COLOR[3]],
                &format!("background pixel {index} at depth {depth}"),
            );
        }
    }

    // Exponential fog never reaches its colour, so the background takes the
    // fog of the far plane's distance.
    let fog = ComponentFog {
        density: 0.01,
        ..fog(FogMode::Exponential)
    };
    let color = fog_color(&fog);
    let output = run_fog(&context, fog, 1.0);
    let (x, y) = (SIZE.0 / 2, SIZE.1 / 2);
    let amount = 1.0 - (-0.01 * distance_along_ray(x, y, 100.0)).exp();
    let expected: [f32; 4] = std::array::from_fn(|channel| match channel {
        3 => COLOR[3],
        _ => COLOR[channel] + (color[channel] - COLOR[channel]) * amount,
    });
    assert_texel(
        output[(y * SIZE.0 + x) as usize],
        expected,
        "the background under exponential fog",
    );
}

#[test]
fn fog_is_off_by_default_and_a_partial_component_loads_with_defaults() {
    assert_eq!(PostProcessSettings::default().fog, None);
    let partial: ComponentFog =
        serde_json::from_str(r#"{ "mode": "Exponential", "density": 0.2 }"#).expect("fog loads");
    assert_eq!(
        partial,
        ComponentFog {
            mode: FogMode::Exponential,
            density: 0.2,
            ..Default::default()
        }
    );
}

/// Creates a PBR material registered as `name` that emits `emission`, so it
/// shows its colour without any light.
fn emissive_material(
    context: &ReadOnlyAssetContext,
    name: &str,
    emission: [f32; 4],
) -> Ref<Material> {
    let registry = context.registries.assets.read();
    let shader = registry
        .load::<Shader>("shaders/mesh/pbr")
        .expect("missing pbr shader");
    let mut material = Material::from_shader(context, shader);
    for variable in material.variables.iter_mut() {
        match variable.name.as_str() {
            "emission_color" => variable.value = ShaderVariableValue::Color(emission),
            "emission_strength" => variable.value = ShaderVariableValue::Float(1.0),
            _ => {}
        }
    }
    registry
        .create_or_update(name.into(), material)
        .expect("material should be created")
}

fn supports_samples(render_context: &RenderContext, samples: u32) -> bool {
    let format = wgpu::TextureFormat::Rgba16Float;
    let features = render_context.device().features();
    let format_features =
        if features.contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
            test_adapter().get_texture_format_features(format)
        } else {
            format.guaranteed_format_features(features)
        };
    format_features.flags.sample_count_supported(samples)
        && format_features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE)
}

/// The pixel `position` lands on in a `size` target.
fn project(camera: &Camera, transform: &Transform, position: Vec3, size: (u32, u32)) -> (u32, u32) {
    let clip = camera.projection
        * transform.inverse_matrix()
        * vec4(position.x, position.y, position.z, 1.0);
    let ndc = clip.xyz() / clip.w;
    (
        ((ndc.x * 0.5 + 0.5) * size.0 as f32) as u32,
        ((0.5 - ndc.y * 0.5) * size.1 as f32) as u32,
    )
}

/// What [`render_two_cubes`] reads back: the display colour of each cube's
/// centre and of the background, sRGB.
struct TwoCubes {
    near: [u8; 4],
    far: [u8; 4],
    background: [u8; 4],
}

/// Renders a red emissive cube 5 units from the camera and a larger one 45
/// units away, through a global volume with `fog`, if any, at `samples`.
fn render_two_cubes(
    context: &ReadOnlyAssetContext,
    samples: u32,
    fog: Option<ComponentFog>,
) -> TwoCubes {
    const VIEW: (u32, u32) = (96, 96);
    let render_context = context.render_context.clone();
    let cube: Ref<Mesh> = context
        .registries
        .assets
        .read()
        .cube()
        .expect("missing cube mesh");
    let material = emissive_material(context, "test/fog_red", [1.0, 0.0, 0.0, 1.0]);
    let near = vec3(-1.2, 0.0, 0.0);
    let far = vec3(12.0, 0.0, 40.0);
    let mut builder = SceneBuilder::new(context.registries.clone());
    for (name, position, scale) in [("near", near, 1.0), ("far", far, 8.0)] {
        let object = builder.spawn(name);
        builder.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(cube.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(
            object,
            glm::translation(&position) * glm::scaling(&vec3(scale, scale, scale)),
        );
    }
    let volume = builder.spawn("volume");
    builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
    if let Some(fog) = fog {
        builder.add_component(volume, fog);
    }
    let scene = builder.finish();
    let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 200.0);
    let transform = Transform::from_components(
        vec3(0.0, 0.0, -5.0),
        UnitQuaternion::face_towards(&vec3(0.0, 0.0, 1.0), &vec3(0.0, 1.0, 0.0)),
        vec3(1.0, 1.0, 1.0),
    );

    let mut renderer = SceneRenderer::new(
        context,
        SceneRendererOptions {
            samples,
            clear_color: Color32::BLACK,
            ..Default::default()
        },
        VIEW,
    )
    .expect("scene renderer should load its assets");
    renderer.render_scene_base(&camera, &transform, &scene, None, None);
    renderer.finalize_scene();
    let mut readback = renderer
        .submit_scene_texture_readback()
        .expect("scene texture has a size");
    let image = loop {
        let _ = render_context.device().poll(wgpu::Maintain::Wait);
        match readback.try_finish(&render_context) {
            ReadbackPoll::Pending => continue,
            ReadbackPoll::Ready(image) => break image,
            ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
        }
    };
    let pixel = |position| {
        let (x, y) = project(&camera, &transform, position, VIEW);
        image.get_pixel(x, y).0
    };
    TwoCubes {
        near: pixel(near),
        far: pixel(far),
        background: image.get_pixel(1, 1).0,
    }
}

/// Euclidean distance between two sRGB colours.
fn color_distance(a: [u8; 4], b: [u8; 4]) -> f32 {
    (0..3)
        .map(|channel| (f32::from(a[channel]) - f32::from(b[channel])).powi(2))
        .sum::<f32>()
        .sqrt()
}

#[test]
fn the_far_surface_is_closer_to_the_fog_colour_than_the_near_one() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let fog = ComponentFog {
        mode: FogMode::Linear,
        color: Color32::from_rgb(0, 0, 255),
        intensity: 1.0,
        start: 0.0,
        end: 60.0,
        opacity: 1.0,
        ..Default::default()
    };
    for samples in [1, 4] {
        if !supports_samples(&context.render_context, samples) {
            continue;
        }
        let clear = render_two_cubes(&context, samples, None);
        assert!(
            color_distance(clear.near, clear.far) <= 3.0,
            "{samples} samples: without fog both cubes look the same: {:?} and {:?}",
            clear.near,
            clear.far
        );
        assert_eq!(
            clear.background,
            [0, 0, 0, 255],
            "and the background stays black"
        );

        let fogged = render_two_cubes(&context, samples, Some(fog));

        let [r, g, b, _] = fogged.background;
        // ACES mixes a little of each channel into the others.
        assert!(
            b > 100 && b > r + 60 && b > g + 60,
            "{samples} samples: the background takes the blue fog colour: {:?}",
            fogged.background
        );
        let near = color_distance(fogged.near, fogged.background);
        let far = color_distance(fogged.far, fogged.background);
        assert!(
            far < near,
            "{samples} samples: the far cube {:?} is closer to the fog colour {:?} than the \
             near one {:?}",
            fogged.far,
            fogged.background,
            fogged.near
        );
        assert!(
            fogged.far[2] > fogged.near[2] && fogged.far[0] < fogged.near[0],
            "{samples} samples: the far cube is bluer and less red than the near one: far {:?}, \
             near {:?}",
            fogged.far,
            fogged.near
        );
    }
}

#[test]
fn the_renderer_runs_fog_first_and_before_depth_of_field_and_bloom() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let renderer = SceneRenderer::new(&context, Default::default(), SIZE)
        .expect("scene renderer should load its assets");

    let hdr: Vec<_> = renderer
        .post_process()
        .passes()
        .iter()
        .filter(|pass| pass.stage() == PostProcessStage::Hdr)
        .map(|pass| pass.name())
        .collect();

    // SSAO is not a chain pass: the lighting pass applies it, before fog.
    assert_eq!(hdr, ["fog", "depth_of_field", "bloom"]);
}
