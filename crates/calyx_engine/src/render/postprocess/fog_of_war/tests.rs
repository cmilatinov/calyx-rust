//! Fog of war: which pixels the pass darkens for a known depth and sight,
//! against the CPU rules of [`FogOfWarSight::pixel_visibility`].

use super::*;
use crate::assets::texture::Texture;
use crate::component::{FogOfWarBlocker, FogOfWarViewer};
use crate::math::Transform;
use crate::render::postprocess::{ChainPlan, ChainTargets, PostProcessChain};
use crate::render::scene_renderer::half_to_f32;
use crate::render::{Camera, RenderContext, SceneRenderer};
use crate::test_support::TestAssets;
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, vec4, Vec2, Vec3};
use std::sync::Arc;

const SIZE: (u32, u32) = (48, 32);
const BYTES_PER_ROW: u32 = 512;
/// Display-linear colour the chain's source is cleared to, with partial
/// alpha.
const COLOR: [f32; 4] = [0.5, 0.25, 0.75, 0.6];
/// Half floats hold about three decimal digits.
const TOLERANCE: f32 = 4e-3;

fn camera() -> Camera {
    Camera::new(
        SIZE.0 as f32 / SIZE.1 as f32,
        60f32.to_radians(),
        0.1,
        200.0,
    )
}

fn planes() -> CameraPlanes {
    let camera = camera();
    CameraPlanes {
        near: camera.near_plane,
        far: camera.far_plane,
        tan_half_fov: CameraPlanes::tan_half_fov_of(&camera.projection),
    }
}

/// A camera at `position` looking at `target`, as a game camera above the
/// ground would.
fn looking(position: Vec3, target: Vec3) -> Transform {
    Transform::from_components(
        position,
        UnitQuaternion::face_towards(&(target - position), &vec3(0.0, 0.0, 1.0)),
        vec3(1.0, 1.0, 1.0),
    )
}

fn fog(sight: Option<FogOfWarSight>) -> ComponentFogOfWar {
    ComponentFogOfWar {
        color: Color32::from_rgb(20, 40, 60),
        opacity: 0.7,
        sight,
    }
}

fn viewer(position: Vec2) -> FogOfWarViewer {
    FogOfWarViewer {
        position,
        fade_start: 6.0,
        fade_end: 16.0,
        fade_curve: 3.0,
        weight: 1.0,
    }
}

/// An axis-aligned blocker of `size` centred on `center`.
fn block(center: Vec2, size: Vec2) -> FogOfWarBlocker {
    FogOfWarBlocker {
        center,
        half_axes: [vec2(size.x * 0.5, 0.0), vec2(0.0, size.y * 0.5)],
    }
}

fn texture(render_context: &Arc<RenderContext>, format: wgpu::TextureFormat) -> Texture {
    Texture::new(
        render_context.clone(),
        &wgpu::TextureDescriptor {
            label: Some("fog_of_war_test_texture"),
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
        label: Some("fog_of_war_test_readback"),
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

/// Runs a chain of the fog of war pass alone, seen from `view`, over a
/// source cleared to [`COLOR`] with the whole scene depth cleared to
/// `depth`, and returns the output, or `None` when the pass did not run.
fn run(
    context: &ReadOnlyAssetContext,
    fog: ComponentFogOfWar,
    view: &Transform,
    depth: f32,
) -> Option<Vec<[f32; 4]>> {
    let render_context = context.render_context.clone();
    let mut chain = PostProcessChain::new(context).expect("chain shaders load");
    chain.push(Box::new(
        FogOfWarPass::new(context).expect("fog of war loads"),
    ));
    chain.set_camera(planes());
    chain.set_view_to_world(view.matrix());
    let source = texture(&render_context, wgpu::TextureFormat::Rgba16Float);
    let output = texture(&render_context, wgpu::TextureFormat::Rgba16Float);
    let scene_depth = texture(&render_context, wgpu::TextureFormat::Depth32Float);
    let mut encoder = render_context
        .device()
        .create_command_encoder(&Default::default());
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("fog_of_war_test_clear"),
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
        &PostProcessSettings::empty().with(fog),
        ChainTargets {
            source: &source,
            output: &output,
            depth: &scene_depth,
            particle_depth: None,
            normals: None,
            albedo: None,
            gbuffer_depth: None,
            material: None,
        },
    );
    if plan.passes.is_empty() {
        return None;
    }
    assert!(plan.depth, "fog of war reads the scene depth");
    render_context.queue().submit(Some(encoder.finish()));
    Some(read_all(&render_context, &output))
}

/// The world position the pixel `(x, y)` shows at depth-buffer value
/// `depth`, seen from `view`, rebuilt as the shader rebuilds it.
fn world_position(view: &Transform, x: u32, y: u32, depth: f32) -> Vec3 {
    let planes = planes();
    let ndc = vec2(
        (x as f32 + 0.5) / SIZE.0 as f32 * 2.0 - 1.0,
        1.0 - (y as f32 + 0.5) / SIZE.1 as f32 * 2.0,
    );
    let z = planes.linear_depth(depth);
    let local = vec4(
        ndc.x * planes.tan_half_fov[0] * z,
        ndc.y * planes.tan_half_fov[1] * z,
        z,
        1.0,
    );
    (view.matrix() * local).xyz()
}

fn to_srgb(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// [`COLOR`] under `fog` where the side sees `seen` of the surface.
fn fogged(fog: &ComponentFogOfWar, seen: f32) -> [f32; 4] {
    let amount = fog.opacity * (1.0 - seen);
    let target = [fog.color.r(), fog.color.g(), fog.color.b()].map(|c| c as f32 / 255.0);
    std::array::from_fn(|channel| match channel {
        3 => COLOR[3],
        _ => {
            let encoded = to_srgb(COLOR[channel]);
            to_linear(encoded + (target[channel] - encoded) * amount)
        }
    })
}

fn assert_texel(actual: [f32; 4], expected: [f32; 4], what: &str) {
    for channel in 0..4 {
        assert!(
            (actual[channel] - expected[channel]).abs() <= TOLERANCE,
            "{what}: expected {expected:?}, got {actual:?}"
        );
    }
}

/// The ground step to a neighbouring pixel, as the shader picks it: the
/// shorter of the steps ahead and behind that stay on the image.
fn ground_step(ahead: Option<Vec2>, behind: Option<Vec2>) -> Vec2 {
    match (ahead, behind) {
        (Some(ahead), Some(behind)) if behind.norm_squared() < ahead.norm_squared() => behind,
        (Some(ahead), _) => ahead,
        (None, Some(behind)) => behind,
        (None, None) => Vec2::zeros(),
    }
}

/// Checks every pixel of `output` against the CPU visibility of the ground
/// footprint it shows, and returns how many came out clear, partly fogged
/// and fully fogged. Pixels with a sample within a few millimetres of an
/// edge of sight, where float rounding may fall either way, are skipped.
fn check_pixels(
    output: &[[f32; 4]],
    fog: &ComponentFogOfWar,
    view: &Transform,
    depth: f32,
) -> [usize; 3] {
    let sight = fog.sight.as_ref().expect("a sight");
    let mut counts = [0; 3];
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let at = |x: u32, y: u32| {
                let world = world_position(view, x, y, depth);
                vec2(world.x, world.z)
            };
            let ground = at(x, y);
            let dx = ground_step(
                (x + 1 < SIZE.0).then(|| at(x + 1, y) - ground),
                (x > 0).then(|| ground - at(x - 1, y)),
            );
            let dy = ground_step(
                (y + 1 < SIZE.1).then(|| at(x, y + 1) - ground),
                (y > 0).then(|| ground - at(x, y - 1)),
            );
            let seen = sight.pixel_visibility(ground, dx, dy);
            let nearby = [
                vec2(0.005, 0.0),
                vec2(-0.005, 0.0),
                vec2(0.0, 0.005),
                vec2(0.0, -0.005),
            ]
            .map(|step| sight.pixel_visibility(ground + step, dx, dy));
            if nearby.iter().any(|other| (other - seen).abs() > 0.05) {
                continue;
            }
            assert_texel(
                output[(y * SIZE.0 + x) as usize],
                fogged(fog, seen),
                &format!("pixel ({x}, {y}) showing {ground:?}, seen {seen}"),
            );
            counts[match seen {
                s if s >= 1.0 => 0,
                s if s > 0.0 => 1,
                _ => 2,
            }] += 1;
        }
    }
    counts
}

#[test]
fn each_pixel_is_fogged_by_the_ground_position_of_the_surface_it_shows() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A camera above the ground, tilted as a game camera is, a second
    // viewer whose sight overlaps the first's, and a block in front of each
    // of them, one turned.
    let view = looking(vec3(-4.0, 14.0, -10.0), vec3(0.0, 0.0, 2.0));
    let turned = std::f32::consts::FRAC_1_SQRT_2;
    let fog = fog(Some(FogOfWarSight {
        viewers: vec![viewer(vec2(-2.0, 0.0)), viewer(vec2(6.0, 6.0))],
        blockers: vec![
            block(vec2(0.5, 1.7), vec2(1.5, 1.5)),
            FogOfWarBlocker {
                center: vec2(4.2, 4.3),
                half_axes: [vec2(turned, turned), vec2(-0.5 * turned, 0.5 * turned)],
            },
        ],
    }));
    let depth = planes().depth_of(16.0);

    let output = run(&context, fog.clone(), &view, depth).expect("the pass runs");

    let [clear, partial, dark] = check_pixels(&output, &fog, &view, depth);
    assert!(
        clear > 20 && partial > 20 && dark > 20,
        "the view shows clear, fading and fully fogged ground: {clear}, {partial}, {dark}"
    );
}

#[test]
fn blockers_all_around_a_viewer_shade_as_the_cpu_rules_say() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // A ring of turned blocks around a viewer, at every bearing, and a run
    // of touching blocks sight carries on through, so the pass reads the
    // blockers sector by sector in every direction.
    let view = looking(vec3(0.0, 22.0, -6.0), vec3(0.0, 0.0, 0.5));
    let mut blockers: Vec<FogOfWarBlocker> = (0..10)
        .map(|step| {
            let bearing = step as f32 * std::f32::consts::TAU / 10.0;
            let turn = step as f32 * 0.6;
            FogOfWarBlocker {
                center: vec2(bearing.cos(), bearing.sin()) * (3.0 + (step % 3) as f32),
                half_axes: [
                    vec2(turn.cos(), turn.sin()) * 0.6,
                    vec2(-turn.sin(), turn.cos()) * 0.3,
                ],
            }
        })
        .collect();
    blockers.extend([
        block(vec2(-1.0, -6.0), vec2(2.0, 1.0)),
        block(vec2(1.0, -6.0), vec2(2.0, 1.0)),
    ]);
    let fog = fog(Some(FogOfWarSight {
        viewers: vec![viewer(vec2(0.2, 0.1))],
        blockers,
    }));
    let depth = planes().depth_of(22.0);

    let output = run(&context, fog.clone(), &view, depth).expect("the pass runs");

    let [clear, partial, dark] = check_pixels(&output, &fog, &view, depth);
    assert!(
        clear > 20 && dark > 20,
        "the view shows ground in sight and in the blocks' shadows: {clear}, {partial}, {dark}"
    );
}

#[test]
fn the_same_pixel_is_clear_on_a_surface_in_sight_and_fogged_on_one_in_shadow() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // Looking along +X from above the viewer; a crate from 3.5 to 4.5 m out
    // casts its shadow along +X. A near surface, such as the top of the
    // crate, shows on the same pixels as the ground in its shadow behind it.
    let view = looking(vec3(-6.0, 10.0, 0.0), vec3(8.0, 0.0, 0.0));
    let fog = fog(Some(FogOfWarSight {
        viewers: vec![viewer(Vec2::zeros())],
        blockers: vec![block(vec2(4.0, 0.0), vec2(1.0, 3.0))],
    }));
    let centre = (SIZE.0 / 2, SIZE.1 / 2);
    let at = |depth: f32| {
        let output = run(&context, fog.clone(), &view, depth).expect("the pass runs");
        let world = world_position(&view, centre.0, centre.1, depth);
        (output[(centre.1 * SIZE.0 + centre.0) as usize], world)
    };

    let (near, near_world) = at(planes().depth_of(12.0));
    let (far, far_world) = at(planes().depth_of(17.0));

    assert!(
        near_world.x < 4.5 && far_world.x > 5.0,
        "the near surface is before the cover's edge and the far one behind it: {near_world:?}, \
         {far_world:?}"
    );
    assert_texel(near, COLOR, "the surface in sight");
    assert_texel(far, fogged(&fog, 0.0), "the surface in shadow");
}

#[test]
fn a_wall_face_turned_from_the_viewer_is_lit_or_fogged_whole() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    // Looking along +X, level with the ground, at the face x = 4 of a block
    // from 4 to 6, which turns from a viewer at x = 8: every pixel of the
    // face rebuilds from depth to the block's edge, give or take rounding.
    let view = looking(vec3(0.0, 0.5, 0.0), vec3(1.0, 0.5, 0.0));
    let depth = planes().depth_of(4.0);
    let face = |hidden: bool| {
        let mut blockers = vec![block(vec2(5.0, 0.0), vec2(2.0, 2.0))];
        if hidden {
            // A block past a gap, between the viewer and the first one.
            blockers.push(block(vec2(7.0, 0.0), vec2(0.5, 4.0)));
        }
        let fog = fog(Some(FogOfWarSight {
            viewers: vec![viewer(vec2(8.0, 0.0))],
            blockers,
        }));
        let output = run(&context, fog.clone(), &view, depth).expect("the pass runs");
        let mut pixels = Vec::new();
        for y in 0..SIZE.1 {
            for x in 0..SIZE.0 {
                let world = world_position(&view, x, y, depth);
                if world.z.abs() < 0.9 {
                    pixels.push((output[(y * SIZE.0 + x) as usize], world, fog.clone()));
                }
            }
        }
        pixels
    };

    let seen = face(false);
    assert!(seen.len() > 100, "the face fills the view: {}", seen.len());
    let fogged_pixels = seen
        .iter()
        .filter(|(texel, ..)| (texel[0] - COLOR[0]).abs() > TOLERANCE)
        .count();
    assert_eq!(
        fogged_pixels,
        0,
        "of {} pixels of the face in sight",
        seen.len()
    );
    for (texel, world, fog) in &seen {
        let ground = vec2(world.x, world.z);
        let sight = fog.sight.as_ref().expect("a sight");
        assert_eq!(sight.visibility(ground), 1.0, "{world:?}");
        assert_texel(*texel, COLOR, &format!("the face in sight at {world:?}"));
    }
    for (texel, world, fog) in &face(true) {
        assert_texel(
            *texel,
            fogged(fog, 0.0),
            &format!("the face in shadow at {world:?}"),
        );
    }
}

#[test]
fn a_side_with_no_viewers_fogs_everything_fully() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let view = looking(vec3(0.0, 10.0, -6.0), Vec3::zeros());
    let fog = fog(Some(FogOfWarSight::default()));

    let output = run(&context, fog.clone(), &view, planes().depth_of(10.0)).expect("runs");

    for (index, texel) in output.into_iter().enumerate() {
        assert_texel(texel, fogged(&fog, 0.0), &format!("pixel {index}"));
    }
}

#[test]
fn without_a_sight_from_game_code_the_pass_does_not_run() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let view = looking(vec3(0.0, 10.0, -6.0), Vec3::zeros());

    assert!(run(&context, fog(None), &view, 0.5).is_none());
    assert!(PostProcessSettings::default()
        .get::<ComponentFogOfWar>()
        .is_none());
}

#[test]
fn the_renderer_fogs_the_inked_image_before_fxaa() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let renderer = SceneRenderer::new(&context, Default::default(), SIZE)
        .expect("scene renderer should load its assets");
    let chain = renderer.post_process();

    let ldr: Vec<_> = chain
        .passes()
        .iter()
        .filter(|pass| pass.stage() == PostProcessStage::Ldr)
        .map(|pass| pass.name())
        .collect();
    assert_eq!(ldr, ["color_grading", "outline", "fog_of_war", "fxaa"]);

    let settings = PostProcessSettings::default().with(fog(Some(FogOfWarSight::default())));
    let plan = ChainPlan::build(chain.passes(), &settings);
    let names: Vec<_> = plan
        .passes
        .iter()
        .map(|planned| chain.passes()[planned.pass].name())
        .collect();
    assert_eq!(names, ["tonemap", "fog_of_war"]);
}

#[test]
fn packing_turns_blockers_into_the_shaders_local_frames() {
    let mut weak = viewer(vec2(1.0, 2.0));
    weak.weight = 0.25;
    let sight = FogOfWarSight {
        viewers: vec![weak, viewer(vec2(-1.0, 0.5))],
        blockers: vec![
            block(vec2(3.0, 4.0), vec2(2.0, 4.0)),
            FogOfWarBlocker {
                center: vec2(9.0, 9.0),
                half_axes: [vec2(1.0, 1.0), vec2(2.0, 2.0)],
            },
        ],
    };
    let mut tables = SightTables {
        viewers: vec![GpuViewer::default(); 5],
        blockers: vec![GpuBlocker::default(); 5],
        ..Default::default()
    };

    pack_sight(&sight, &mut tables);

    assert_eq!(
        tables
            .viewers
            .iter()
            .map(|viewer| (viewer.position, viewer.weight, viewer.first_sector))
            .collect::<Vec<_>>(),
        [
            ([1.0, 2.0], 0.25, 0),
            ([-1.0, 0.5], 1.0, SIGHT_SECTORS as u32)
        ]
    );
    assert_eq!(tables.viewers[0].fade_curve, 3.0);
    assert_eq!(tables.sectors.len(), 2 * SIGHT_SECTORS);
    // The flat second blocker blocks nothing and is left out.
    assert_eq!(
        tables.blockers,
        [GpuBlocker {
            center: [3.0, 4.0],
            local_x: [1.0, 0.0],
            local_y: [0.0, 0.5],
            scale: [1.0, 0.5],
        }]
    );
}

/// The sectors of the viewer `viewer` of `tables` that list blocker
/// `blocker`.
fn sectors_listing(tables: &SightTables, viewer: usize, blocker: u32) -> Vec<usize> {
    let first = tables.viewers[viewer].first_sector as usize;
    (0..SIGHT_SECTORS)
        .filter(|&sector| {
            let [start, count] = tables.sectors[first + sector];
            tables.sector_blockers[start as usize..(start + count) as usize].contains(&blocker)
        })
        .collect()
}

#[test]
fn each_viewer_lists_a_blocker_in_the_sectors_its_angular_span_overlaps() {
    let sight = FogOfWarSight {
        viewers: vec![viewer(vec2(0.0, 0.0)), viewer(vec2(10.0, 0.0))],
        // A post 0.2 units across, 10 units east of the first viewer: on the
        // +x axis, where the first sector starts, so it spans the last
        // sector too, and narrower than a sector.
        blockers: vec![block(vec2(10.0, 0.0), vec2(0.2, 0.2))],
    };
    let mut tables = SightTables::default();

    pack_sight(&sight, &mut tables);

    assert_eq!(sectors_listing(&tables, 0, 0), [0, SIGHT_SECTORS - 1]);
    assert_eq!(
        sectors_listing(&tables, 1, 0),
        (0..SIGHT_SECTORS).collect::<Vec<_>>(),
        "a viewer standing in a blocker sees it in every direction"
    );
}

#[test]
fn every_line_of_sight_through_a_blocker_lies_in_a_sector_listing_it() {
    // Blocks of several sizes and turns, at several distances and bearings
    // from the viewer; each line from the viewer through points of a block
    // must point into a sector that lists it.
    let origin = vec2(0.3, -0.7);
    let mut blockers = Vec::new();
    for ring in 1..6 {
        for step in 0..24 {
            let bearing = step as f32 * std::f32::consts::TAU / 24.0 + ring as f32 * 0.1;
            let distance = ring as f32 * 3.0;
            let turn = step as f32 * 0.37;
            let (along, across) = (0.4 + 0.3 * ring as f32, 0.2 + 0.1 * (step % 4) as f32);
            blockers.push(FogOfWarBlocker {
                center: origin + vec2(bearing.cos(), bearing.sin()) * distance,
                half_axes: [
                    vec2(turn.cos(), turn.sin()) * along,
                    vec2(-turn.sin(), turn.cos()) * across,
                ],
            });
        }
    }
    let sight = FogOfWarSight {
        viewers: vec![viewer(origin)],
        blockers: blockers.clone(),
    };
    let mut tables = SightTables::default();

    pack_sight(&sight, &mut tables);

    let sector_angle = std::f32::consts::TAU / SIGHT_SECTORS as f32;
    for (index, blocker) in blockers.iter().enumerate() {
        let listed = sectors_listing(&tables, 0, index as u32);
        assert!(
            listed.len() < SIGHT_SECTORS / 2,
            "a block away from the viewer spans part of the circle: {listed:?}"
        );
        for u in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
            for v in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                let point = blocker.center + blocker.half_axes[0] * u + blocker.half_axes[1] * v;
                let direction = point - origin;
                let angle = direction
                    .y
                    .atan2(direction.x)
                    .rem_euclid(std::f32::consts::TAU);
                let sector = ((angle / sector_angle).floor() as usize).min(SIGHT_SECTORS - 1);
                assert!(
                    listed.contains(&sector),
                    "block {index}: the line to {point:?} points into sector {sector}, \
                     not in {listed:?}"
                );
            }
        }
    }
}

#[test]
fn the_uniform_viewer_and_blocker_match_the_shaders_layout() {
    let source = crate::render::test_kernel::preprocess(
        "//#include \"shaders/postprocess/fog_of_war.wgsl\"\n",
    )
    .expect("the fog of war shader preprocesses");
    let module = naga::front::wgsl::parse_str(&source).expect("the fog of war shader parses");

    let (size, members) = crate::render::test_kernel::wgsl_layout(&module, "FogOfWarUniform");
    assert_eq!(size as usize, size_of::<FogOfWarUniform>());
    assert_eq!(
        members[..6],
        [
            ("color", 0),
            ("camera", 16),
            ("view_to_world", 32),
            ("viewer_count", 96),
            ("blocker_count", 100),
            ("sector_count", 104),
        ]
        .map(|(name, offset)| (name.to_string(), offset))
    );
    let (size, members) = crate::render::test_kernel::wgsl_layout(&module, "Viewer");
    assert_eq!(size as usize, size_of::<GpuViewer>());
    assert_eq!(
        members,
        [
            ("position", 0),
            ("fade_start", 8),
            ("fade_end", 12),
            ("fade_curve", 16),
            ("weight", 20),
            ("first_sector", 24),
            ("_pad", 28),
        ]
        .map(|(name, offset)| (name.to_string(), offset))
    );
    let (size, members) = crate::render::test_kernel::wgsl_layout(&module, "Blocker");
    assert_eq!(size as usize, size_of::<GpuBlocker>());
    assert_eq!(
        members,
        [
            ("center", 0),
            ("local_x", 8),
            ("local_y", 16),
            ("scale", 24),
        ]
        .map(|(name, offset)| (name.to_string(), offset))
    );
}
