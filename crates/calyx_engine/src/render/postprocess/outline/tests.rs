//! Ink outlines: where a rendered scene gets lines, how wide they are, and
//! which style draws them.

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
/// The ink of style 1: pure green, which neither the red surfaces nor the
/// black background ever show.
const INK: [u8; 4] = [0, 255, 0, 255];
/// The ink of style 2: pure blue.
const BLUE_INK: [u8; 4] = [0, 0, 255, 255];

/// A style of `ink` with silhouettes `width` pixels wide and no inner
/// lines.
fn style(ink: [u8; 4], width: f32) -> OutlineStyle {
    OutlineStyle {
        color: Color32::from_rgb(ink[0], ink[1], ink[2]),
        width,
        inner_width: 1.0,
        inner_opacity: 0.0,
        opacity: 1.0,
    }
}

/// An outline effect with `styles`, at the default thresholds.
fn outline(styles: Vec<OutlineStyle>) -> ComponentOutline {
    ComponentOutline {
        styles,
        ..Default::default()
    }
}

/// Creates a PBR material registered as `name` that emits pure red, so it
/// shows its colour without any light, with outline style `outline_style`.
fn red_material(context: &ReadOnlyAssetContext, name: &str, outline_style: u32) -> Ref<Material> {
    let registry = context.registries.assets.read();
    let shader = registry
        .load::<Shader>("shaders/mesh/pbr")
        .expect("missing pbr shader");
    let mut material = Material::from_shader(context, shader);
    for (variable, value) in [
        (
            "base_color_factor",
            ShaderVariableValue::Color([0.0, 0.0, 0.0, 1.0]),
        ),
        (
            "emission_color",
            ShaderVariableValue::Color([1.0, 0.0, 0.0, 1.0]),
        ),
        ("emission_strength", ShaderVariableValue::Float(1.0)),
        ("outline_style", ShaderVariableValue::Uint(outline_style)),
    ] {
        assert!(material.set_variable(variable, value), "{variable}");
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

/// A red emissive mesh of `scale` at `position`, outlined in `style`.
struct Shape {
    mesh: Ref<Mesh>,
    position: Vec3,
    scale: Vec3,
    style: u32,
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
    let mut builder = SceneBuilder::new(context.registries.clone());
    for (index, shape) in shapes.iter().enumerate() {
        let material = red_material(
            context,
            &format!("test/outline_red_{}", shape.style),
            shape.style,
        );
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

/// Any pixel of `image` tinted by some ink: neither the red surfaces nor the
/// black background.
fn is_inked(image: &image::RgbaImage, (x, y): (u32, u32)) -> bool {
    let [_, g, b, _] = image.get_pixel(x, y).0;
    g >= 60 || b >= 60
}

/// The pixels along row `line` (or column, when `column` is set) of `image`
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

fn inked_count(image: &image::RgbaImage) -> usize {
    image
        .enumerate_pixels()
        .filter(|(x, y, _)| is_inked(image, (*x, *y)))
        .count()
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

/// A unit cube at the origin in `style`.
fn unit_cube(context: &ReadOnlyAssetContext, style: u32) -> Shape {
    Shape {
        mesh: cube(context),
        position: Vec3::zeros(),
        scale: Vec3::repeat(1.0),
        style,
    }
}

/// A ground slab in `style` whose top, at y = 0, spans far past the view.
fn ground(context: &ReadOnlyAssetContext, style: u32) -> Shape {
    Shape {
        mesh: cube(context),
        position: vec3(0.0, -0.5, 0.0),
        scale: vec3(120.0, 1.0, 120.0),
        style,
    }
}

#[test]
fn a_cube_against_the_background_gets_a_line_of_the_set_width_around_it() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let shapes = [unit_cube(&context, 1)];
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
                Some(outline(vec![style(INK, width as f32)])),
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
fn style_zero_draws_nothing_and_a_style_past_the_list_draws_nothing_and_warns() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let camera = camera_at(vec3(0.0, 0.0, -4.0), Vec3::zeros());

    for samples in sample_counts(&context) {
        let unoutlined = render(
            &context,
            &[unit_cube(&context, 0)],
            &camera,
            samples,
            Some(outline(vec![style(INK, 2.0)])),
        );
        assert_eq!(
            inked_count(&unoutlined),
            0,
            "{samples} samples: a material of style 0 is not outlined"
        );

        let missing = render(
            &context,
            &[unit_cube(&context, 2)],
            &camera,
            samples,
            Some(outline(vec![style(INK, 2.0)])),
        );
        assert_eq!(
            inked_count(&missing),
            0,
            "{samples} samples: style 2 of a one-style list draws no line"
        );
    }
    let material = context
        .registries
        .assets
        .read()
        .asset_id("test/outline_red_2")
        .expect("the style 2 material is registered");
    assert!(
        crate::logging::has_warned(&format!("outline-style:{material}:2")),
        "the renderer warns about the material and the missing style"
    );
}

#[test]
fn an_edge_takes_the_nearer_surfaces_style_or_the_farther_ones_when_it_has_none() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A cube in front of a wall, which fills the view behind it.
    let wall = |style| Shape {
        mesh: cube(&context),
        position: vec3(0.0, 0.0, 6.0),
        scale: vec3(40.0, 40.0, 1.0),
        style,
    };
    let camera = camera_at(vec3(0.0, 0.0, -4.0), Vec3::zeros());
    let middle = project(&camera, Vec3::zeros());
    let styles = vec![style(INK, 2.0), style(BLUE_INK, 2.0)];
    let inks = |image: &image::RgbaImage| {
        let mut inks: Vec<[u8; 4]> = (0..VIEW.0)
            .map(|x| image.get_pixel(x, middle.1).0)
            .filter(|pixel| *pixel == INK || *pixel == BLUE_INK)
            .collect();
        inks.dedup();
        inks
    };

    for samples in sample_counts(&context) {
        let near_wins = render(
            &context,
            &[unit_cube(&context, 1), wall(2)],
            &camera,
            samples,
            Some(outline(styles.clone())),
        );
        assert_eq!(
            inks(&near_wins),
            [INK],
            "{samples} samples: the cube's silhouette draws in the cube's style"
        );

        let fallback = render(
            &context,
            &[unit_cube(&context, 0), wall(2)],
            &camera,
            samples,
            Some(outline(styles.clone())),
        );
        assert_eq!(
            inks(&fallback),
            [BLUE_INK],
            "{samples} samples: a cube of style 0 is outlined in the wall's style"
        );
    }
}

#[test]
fn a_change_of_style_between_touching_surfaces_is_an_edge() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // Two cubes side by side, touching at x = 0, their front faces in one
    // plane facing the camera.
    let pair = |left, right| {
        [
            Shape {
                mesh: cube(&context),
                position: vec3(-0.5, 0.0, 0.0),
                scale: Vec3::repeat(1.0),
                style: left,
            },
            Shape {
                mesh: cube(&context),
                position: vec3(0.5, 0.0, 0.0),
                scale: Vec3::repeat(1.0),
                style: right,
            },
        ]
    };
    let camera = camera_at(vec3(0.0, 0.0, -4.0), Vec3::zeros());
    let seam = project(&camera, vec3(0.0, 0.0, -0.5));
    let styles = vec![style(INK, 2.0), style(INK, 2.0)];

    for samples in sample_counts(&context) {
        let same = render(
            &context,
            &pair(1, 1),
            &camera,
            samples,
            Some(outline(styles.clone())),
        );
        assert!(
            !(seam.0 - 2..=seam.0 + 2).any(|x| is_inked(&same, (x, seam.1))),
            "{samples} samples: one style across the seam draws no line"
        );

        let changed = render(
            &context,
            &pair(1, 2),
            &camera,
            samples,
            Some(outline(styles.clone())),
        );
        let line: Vec<u32> = (seam.0 - 3..=seam.0 + 3)
            .filter(|&x| is_ink(&changed, (x, seam.1)))
            .collect();
        assert_eq!(
            line.len(),
            2,
            "{samples} samples: a change of style draws a silhouette along the seam: {line:?}"
        );
    }
}

#[test]
fn a_flat_plane_gets_no_line() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // The ground seen from 2 units up: steeply, and at a grazing angle whose
    // top row meets the ground about 23 units away.
    let shapes = [ground(&context, 1)];
    for pitch in [60f32, 35.0] {
        let position = vec3(0.0, 2.0, 0.0);
        let forward = vec3(0.0, -pitch.to_radians().sin(), pitch.to_radians().cos());
        let camera = camera_at(position, position + forward);
        for samples in sample_counts(&context) {
            let mut wide = style(INK, 3.0);
            wide.inner_width = 3.0;
            wide.inner_opacity = 1.0;
            let image = render(
                &context,
                &shapes,
                &camera,
                samples,
                Some(outline(vec![wide])),
            );
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
                inked_count(&image),
                0,
                "{samples} samples, pitch {pitch}: the ground draws no line"
            );
        }
    }
}

#[test]
fn a_crease_draws_an_inner_line_and_the_silhouette_a_full_one() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A box standing on the ground, both of one style: the box's front face
    // meets the ground's top in a crease along X with no jump in distance,
    // and the box's top edge stands out against the ground far behind it.
    let shapes = [
        ground(&context, 1),
        Shape {
            mesh: cube(&context),
            position: vec3(0.0, 0.5, 0.0),
            scale: vec3(2.0, 1.0, 1.0),
            style: 1,
        },
    ];
    let crease = vec3(0.0, 0.0, -0.5);
    let camera = camera_at(vec3(0.0, 2.5, -3.0), crease);
    let style = OutlineStyle {
        inner_width: 3.0,
        inner_opacity: 0.5,
        ..style(INK, 2.0)
    };

    for samples in sample_counts(&context) {
        let image = render(
            &context,
            &shapes,
            &camera,
            samples,
            Some(outline(vec![style])),
        );

        // Down the middle column: the crease, then the top's far edge.
        let (x, y) = project(&camera, crease);
        let inner: Vec<u32> = (y - 4..=y + 4)
            .filter(|&y| is_inked(&image, (x, y)))
            .collect();
        assert_eq!(
            inner.len(),
            3,
            "{samples} samples: the crease draws an inner line 3 pixels wide: {inner:?}"
        );
        for &y in &inner {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            // Half of the ink over the red: neither red nor the ink.
            assert!(
                g > 120 && g < 230 && r > 60 && b < 60,
                "{samples} samples: the crease takes half of the ink at ({x}, {y}): {:?}",
                [r, g, b]
            );
        }
        let (_, top) = project(&camera, vec3(0.0, 1.0, 0.5));
        let silhouette: Vec<u32> = (top - 4..=top + 4)
            .filter(|&y| is_ink(&image, (x, y)))
            .collect();
        assert_eq!(
            silhouette.len(),
            2,
            "{samples} samples: the box's far top edge is a full silhouette 2 pixels wide"
        );
        for (point, what) in [
            (vec3(0.0, 0.5, -0.5), "the box's front face"),
            (vec3(0.0, 0.0, -1.5), "the ground in front of the box"),
            (vec3(0.0, 1.0, 0.0), "the box's top"),
        ] {
            assert!(
                !is_inked(&image, project(&camera, point)),
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
        style: 1,
    }];
    let camera = camera_at(vec3(0.0, 0.0, -3.0), Vec3::zeros());
    let middle = project(&camera, Vec3::zeros());
    let mut ink = style(INK, 2.0);
    ink.inner_opacity = 1.0;

    for samples in sample_counts(&context) {
        let plain = render(&context, &shapes, &camera, samples, None);
        let outlined = render(
            &context,
            &shapes,
            &camera,
            samples,
            Some(outline(vec![ink])),
        );

        let red = along(&plain, middle.1, false, is_red);
        let (first, last) = (red[0], *red.last().unwrap());
        assert_eq!(
            along(&outlined, middle.1, false, is_inked),
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
        serde_json::from_str(r#"{ "styles": [{ "width": 3.0 }], "normal_threshold": 30.0 }"#)
            .expect("outline loads");
    assert_eq!(
        partial,
        ComponentOutline {
            styles: vec![OutlineStyle {
                width: 3.0,
                ..Default::default()
            }],
            normal_threshold: 30.0,
            ..Default::default()
        }
    );
    assert!(
        ComponentOutline::default().styles.is_empty(),
        "no built-in style"
    );
    assert_eq!(
        OutlineStyle::default().color,
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
        assert_eq!(outline_pixel_width(width), pixels, "width {width}");
    }
}

#[test]
fn only_the_first_eight_styles_are_drawn() {
    let many = outline(vec![OutlineStyle::default(); MAX_OUTLINE_STYLES + 2]);

    assert_eq!(drawn_outline_styles(&many).len(), MAX_OUTLINE_STYLES);
    assert!(crate::logging::has_warned("outline-styles-over-max"));
    let few = outline(vec![OutlineStyle::default(); 3]);
    assert_eq!(drawn_outline_styles(&few).len(), 3);
}

#[test]
fn the_uniform_matches_the_shaders_layout() {
    let source =
        crate::render::test_kernel::preprocess("//#include \"shaders/postprocess/outline.wgsl\"\n")
            .expect("the outline shader preprocesses");
    let module = naga::front::wgsl::parse_str(&source).expect("the outline shader parses");

    let (size, members) = crate::render::test_kernel::wgsl_layout(&module, "InkOutlineUniform");
    assert_eq!(size as usize, size_of::<InkOutlineUniform>());
    assert_eq!(
        members,
        [
            ("camera", 0),
            ("style_count", 16),
            ("max_width", 20),
            ("depth_threshold", 24),
            ("silhouette_threshold", 28),
            ("normal_cos", 32),
            ("styles", 48),
        ]
        .map(|(name, offset)| (name.to_string(), offset))
    );
    let (size, _) = crate::render::test_kernel::wgsl_layout(&module, "InkStyle");
    assert_eq!(size as usize, size_of::<InkStyle>());
}
