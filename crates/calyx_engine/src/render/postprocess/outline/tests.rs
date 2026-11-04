//! Ink outlines: where a rendered scene gets lines, and how wide they are.

use super::*;
use crate::assets::material::{Material, ShaderVariableValue};
use crate::assets::mesh::Mesh;
use crate::assets::AssetRef;
use crate::component::{ComponentGlobalPostProcessVolume, ComponentMesh};
use crate::math::Transform;
use crate::render::{Camera, ReadbackPoll, RenderContext, SceneRenderer, SceneRendererOptions};
use crate::test_support::{test_adapter, SceneBuilder, TestAssets};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{self as glm, vec3, vec4, Vec3};

const VIEW: (u32, u32) = (96, 96);
/// The ink the tests draw with: pure green, which neither the red surfaces
/// nor the black background ever show.
const INK: [u8; 4] = [0, 255, 0, 255];

fn outline(width: f32) -> ComponentOutline {
    ComponentOutline {
        color: Color32::from_rgb(INK[0], INK[1], INK[2]),
        width,
        ..Default::default()
    }
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
            "base_color_factor" => {
                variable.value = ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0])
            }
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

/// A camera at `position` looking at `target`, 60 degrees wide.
fn camera_at(position: Vec3, target: Vec3) -> (Camera, Transform) {
    let camera = Camera::new(1.0, 60f32.to_radians(), 0.1, 200.0);
    let transform = Transform::from_components(
        position,
        UnitQuaternion::face_towards(&(target - position), &Vec3::y()),
        vec3(1.0, 1.0, 1.0),
    );
    (camera, transform)
}

/// The pixel `position` lands on.
fn project(camera: &(Camera, Transform), position: Vec3) -> (u32, u32) {
    let clip = camera.0.projection
        * camera.1.inverse_matrix()
        * vec4(position.x, position.y, position.z, 1.0);
    let ndc = clip.xyz() / clip.w;
    (
        ((ndc.x * 0.5 + 0.5) * VIEW.0 as f32) as u32,
        ((0.5 - ndc.y * 0.5) * VIEW.1 as f32) as u32,
    )
}

/// A red emissive box of `scale` at `position`.
struct Shape {
    mesh: Ref<Mesh>,
    position: Vec3,
    scale: Vec3,
}

/// Renders `shapes`, red and emissive over a black background, from
/// `camera` at `samples`, through a global volume with `outline`, if any,
/// and reads the display colour back.
fn render(
    context: &ReadOnlyAssetContext,
    shapes: &[Shape],
    camera: &(Camera, Transform),
    samples: u32,
    outline: Option<ComponentOutline>,
) -> image::RgbaImage {
    let render_context = context.render_context.clone();
    let material = emissive_material(context, "test/outline_red", [1.0, 0.0, 0.0, 1.0]);
    let mut builder = SceneBuilder::new(context.registries.clone());
    for (index, shape) in shapes.iter().enumerate() {
        let object = builder.spawn(&format!("shape {index}"));
        builder.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(shape.mesh.id()),
                material: AssetRef::from_id(material.id()),
                ..Default::default()
            },
        );
        builder.set_transform(
            object,
            glm::translation(&shape.position) * glm::scaling(&shape.scale),
        );
    }
    let volume = builder.spawn("volume");
    builder.add_component(volume, ComponentGlobalPostProcessVolume::default());
    if let Some(outline) = outline {
        builder.add_component(volume, outline);
    }
    let scene = builder.finish();

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
    renderer.render_scene_base(&camera.0, &camera.1, &scene, None, None);
    renderer.finalize_scene();
    let mut readback = renderer
        .submit_scene_texture_readback()
        .expect("scene texture has a size");
    loop {
        let _ = render_context.device().poll(wgpu::Maintain::Wait);
        match readback.try_finish(&render_context) {
            ReadbackPoll::Pending => continue,
            ReadbackPoll::Ready(image) => break image,
            ReadbackPoll::Failed(error) => panic!("scene readback failed: {error}"),
        }
    }
}

fn is_ink(image: &image::RgbaImage, (x, y): (u32, u32)) -> bool {
    image.get_pixel(x, y).0 == INK
}

fn is_red(image: &image::RgbaImage, (x, y): (u32, u32)) -> bool {
    let [r, g, b, _] = image.get_pixel(x, y).0;
    r > 100 && g < 60 && b < 60
}

/// The pixels along row `y` (or column, when `column` is set) of `image`
/// that pass `test`.
fn along(
    image: &image::RgbaImage,
    line: u32,
    column: bool,
    test: fn(&image::RgbaImage, (u32, u32)) -> bool,
) -> Vec<u32> {
    let length = if column { VIEW.1 } else { VIEW.0 };
    (0..length)
        .filter(|&at| test(image, if column { (line, at) } else { (at, line) }))
        .collect()
}

fn sample_counts(context: &ReadOnlyAssetContext) -> Vec<u32> {
    [1, 4]
        .into_iter()
        .filter(|&samples| supports_samples(&context.render_context, samples))
        .collect()
}

fn cube(context: &ReadOnlyAssetContext) -> Ref<Mesh> {
    context
        .registries
        .assets
        .read()
        .cube()
        .expect("missing cube mesh")
}

#[test]
fn a_cube_against_the_background_gets_a_line_of_the_set_width_around_it() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let shapes = [Shape {
        mesh: cube(&context),
        position: Vec3::zeros(),
        scale: Vec3::repeat(1.0),
    }];
    let camera = camera_at(vec3(0.0, 0.0, -4.0), Vec3::zeros());
    let middle = project(&camera, Vec3::zeros());

    for samples in sample_counts(&context) {
        let plain = render(&context, &shapes, &camera, samples, None);
        assert!(
            along(&plain, middle.1, false, is_ink).is_empty(),
            "{samples} samples: no lines without the effect"
        );
        for width in [1u32, 2, 3, 4] {
            let outlined = render(
                &context,
                &shapes,
                &camera,
                samples,
                Some(outline(width as f32)),
            );
            // Across the middle row, then down the middle column.
            for (line, column) in [(middle.1, false), (middle.0, true)] {
                let red = along(&plain, line, column, is_red);
                let (first, last) = (red[0], *red.last().unwrap());
                assert_eq!(
                    red.len() as u32,
                    last - first + 1,
                    "the cube's face is one run"
                );
                // The line straddles each edge; the cube is nearer than the
                // background, so it takes the odd pixel.
                let inside = width.div_ceil(2);
                let outside = width / 2;
                let expected: Vec<u32> = (first - outside..first + inside)
                    .chain(last + 1 - inside..last + 1 + outside)
                    .collect();
                assert_eq!(
                    along(&outlined, line, column, is_ink),
                    expected,
                    "{samples} samples, width {width}, {} {line}: the face spans {first}..={last}",
                    if column { "column" } else { "row" }
                );
            }
            assert!(
                !is_ink(&outlined, middle),
                "{samples} samples, width {width}: the flat face draws no line"
            );
        }
    }
}

#[test]
fn a_flat_plane_gets_no_line() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A ground slab whose top, at y = 0, fills the view, seen from 2 units
    // up: steeply, and at a grazing angle whose top row meets the ground
    // about 23 units away.
    let shapes = [Shape {
        mesh: cube(&context),
        position: vec3(0.0, -0.5, 0.0),
        scale: vec3(120.0, 1.0, 120.0),
    }];
    for pitch in [60f32, 35.0] {
        let position = vec3(0.0, 2.0, 0.0);
        let forward = vec3(0.0, -pitch.to_radians().sin(), pitch.to_radians().cos());
        let camera = camera_at(position, position + forward);
        for samples in sample_counts(&context) {
            let image = render(&context, &shapes, &camera, samples, Some(outline(3.0)));
            let ink = image.pixels().filter(|pixel| pixel.0 == INK).count();
            let red = image
                .enumerate_pixels()
                .filter(|(x, y, _)| is_red(&image, (*x, *y)))
                .count();
            assert_eq!(
                red,
                (VIEW.0 * VIEW.1) as usize,
                "pitch {pitch}: the ground fills the view"
            );
            assert_eq!(
                ink, 0,
                "{samples} samples, pitch {pitch}: the ground draws no line"
            );
        }
    }
}

#[test]
fn a_crease_between_two_boxes_gets_a_line() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A box standing on a ground slab: its front face meets the ground's top
    // in a crease along X, with no jump in distance across it.
    let shapes = [
        Shape {
            mesh: cube(&context),
            position: vec3(0.0, -0.5, 0.0),
            scale: vec3(120.0, 1.0, 120.0),
        },
        Shape {
            mesh: cube(&context),
            position: vec3(0.0, 0.5, 0.0),
            scale: vec3(2.0, 1.0, 1.0),
        },
    ];
    let crease = vec3(0.0, 0.0, -0.5);
    let camera = camera_at(vec3(0.0, 2.5, -3.0), crease);

    for samples in sample_counts(&context) {
        let image = render(&context, &shapes, &camera, samples, Some(outline(3.0)));

        let (x, y) = project(&camera, crease);
        assert!(
            (y - 1..=y + 1).any(|y| is_ink(&image, (x, y))),
            "{samples} samples: the crease draws a line"
        );
        for (point, what) in [
            (vec3(0.0, 0.5, -0.5), "the box's front face"),
            (vec3(0.0, 0.0, -1.5), "the ground in front of the box"),
            (vec3(0.0, 1.0, 0.0), "the box's top"),
        ] {
            assert!(
                !is_ink(&image, project(&camera, point)),
                "{samples} samples: {what} draws no line"
            );
        }
    }
}

#[test]
fn a_smooth_sphere_gets_its_silhouette_and_no_line_across_its_curve() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let sphere = context
        .registries
        .assets
        .read()
        .sphere()
        .expect("missing sphere mesh");
    let shapes = [Shape {
        mesh: sphere,
        position: Vec3::zeros(),
        scale: Vec3::repeat(1.0),
    }];
    let camera = camera_at(vec3(0.0, 0.0, -3.0), Vec3::zeros());
    let middle = project(&camera, Vec3::zeros());

    for samples in sample_counts(&context) {
        let plain = render(&context, &shapes, &camera, samples, None);
        let outlined = render(&context, &shapes, &camera, samples, Some(outline(2.0)));

        let red = along(&plain, middle.1, false, is_red);
        let (first, last) = (red[0], *red.last().unwrap());
        assert_eq!(
            along(&outlined, middle.1, false, is_ink),
            [first - 1, first, last, last + 1],
            "{samples} samples: only the silhouette draws a line across the middle row"
        );
    }
}

#[test]
fn the_renderer_draws_outlines_after_colour_grading_and_before_fxaa() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let renderer = SceneRenderer::new(&context, Default::default(), VIEW)
        .expect("scene renderer should load its assets");

    let ldr: Vec<_> = renderer
        .post_process()
        .passes()
        .iter()
        .filter(|pass| pass.stage() == PostProcessStage::Ldr)
        .map(|pass| pass.name())
        .collect();

    assert_eq!(ldr, ["color_grading", "outline", "fxaa"]);
}

#[test]
fn outlines_are_off_by_default_and_a_partial_component_loads_with_defaults() {
    assert_eq!(PostProcessSettings::default().outline, None);
    let partial: ComponentOutline =
        serde_json::from_str(r#"{ "width": 3.0, "normal_threshold": 30.0 }"#)
            .expect("outline loads");
    assert_eq!(
        partial,
        ComponentOutline {
            width: 3.0,
            normal_threshold: 30.0,
            ..Default::default()
        }
    );
    assert_eq!(
        ComponentOutline::default().color,
        Color32::from_rgb(0x12, 0x12, 0x12),
        "the style guide's ink"
    );
}

#[test]
fn the_width_rounds_to_whole_pixels_within_the_shaders_reach() {
    for (width, pixels) in [
        (0.0, 1),
        (1.4, 1),
        (1.6, 2),
        (2.5, 3),
        (8.0, 8),
        (20.0, 8),
        (f32::NAN, 1),
    ] {
        assert_eq!(
            outline_pixel_width(&outline(width)),
            pixels,
            "width {width}"
        );
    }
}
