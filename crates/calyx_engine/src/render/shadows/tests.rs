use super::*;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, vec4};

fn settings(distance: f32) -> ShadowSettings {
    ShadowSettings {
        distance,
        depth_bias: 1.0,
        normal_bias: 1.0,
    }
}

fn camera_at(position: Vec3, forward: Vec3) -> ShadowCamera {
    let camera = Camera::new(16.0 / 9.0, 70f32.to_radians(), 0.1, 1000.0);
    let transform = Transform::from_components(
        position,
        UnitQuaternion::face_towards(&forward, &Vec3::y()),
        vec3(1.0, 1.0, 1.0),
    );
    ShadowCamera::new(&camera, &transform)
}

fn spot(position: Vec3, distance: f32) -> Option<ShadowCaster> {
    Some(ShadowCaster::Spot {
        position,
        direction: -Vec3::y(),
        range: 10.0,
        outer_angle: 30f32.to_radians(),
        settings: settings(distance),
    })
}

fn sun(distance: f32) -> Option<ShadowCaster> {
    Some(ShadowCaster::Directional {
        direction: vec3(1.0, -2.0, 0.5).normalize(),
        settings: settings(distance),
    })
}

fn clip(view_projection: &Mat4, point: Vec3) -> Vec3 {
    let clip = view_projection * vec4(point.x, point.y, point.z, 1.0);
    clip.xyz() / clip.w
}

#[test]
fn splits_blend_even_and_logarithmic_and_end_at_the_shadow_distance() {
    let splits = cascade_splits(0.1, 50.0);
    assert!((splits[SHADOW_CASCADE_COUNT - 1] - 50.0).abs() < 1e-3);
    assert!(splits.windows(2).all(|pair| pair[0] < pair[1]));
    for (i, split) in splits.iter().enumerate() {
        let fraction = (i + 1) as f32 / SHADOW_CASCADE_COUNT as f32;
        let logarithmic = 0.1 * (50.0f32 / 0.1).powf(fraction);
        let even = 0.1 + 49.9 * fraction;
        assert!(
            *split > logarithmic.min(even) - 1e-4 && *split < logarithmic.max(even) + 1e-4,
            "split {i} lies between the even and logarithmic splits: {split}"
        );
    }
}

#[test]
fn the_first_shadowed_directional_light_gets_the_cascades() {
    let camera = camera_at(Vec3::zeros(), Vec3::z());
    let (plan, indices) = ShadowPlan::new(&[None, sun(40.0), sun(20.0)], Some(&camera));
    assert_eq!(indices, [-1, 0, -1]);
    let cascades = plan.directional.expect("the first sun has cascades");
    assert_eq!(cascades.settings.distance, 40.0);
    assert!((cascades.split_far[SHADOW_CASCADE_COUNT - 1] - 40.0).abs() < 1e-3);
}

#[test]
fn without_a_camera_no_light_gets_a_shadow() {
    let (plan, indices) = ShadowPlan::new(&[sun(40.0), spot(Vec3::zeros(), 50.0)], None);
    assert!(plan.is_empty());
    assert_eq!(indices, [-1, -1]);
}

#[test]
fn each_cascade_holds_its_slice_and_the_blend_band_before_it() {
    // `directional_shadow` blends a point over the last `CASCADE_BLEND`
    // of cascade i - 1, measured from the split before that (0 for the
    // first), into cascade i, so cascade i must hold that band too. A
    // narrow field of view makes the slices short and wide, where a fit
    // from the split alone misses part of the band.
    let narrow = {
        let camera = Camera::new(16.0 / 9.0, 10f32.to_radians(), 0.1, 1000.0);
        let transform = Transform::from_components(
            Vec3::zeros(),
            UnitQuaternion::face_towards(&Vec3::z(), &Vec3::y()),
            vec3(1.0, 1.0, 1.0),
        );
        ShadowCamera::new(&camera, &transform)
    };
    let wide = camera_at(vec3(3.0, 2.0, -4.0), vec3(0.3, -0.2, 1.0).normalize());
    let side_sun = Some(ShadowCaster::Directional {
        direction: Vec3::x(),
        settings: settings(50.0),
    });
    for (name, camera, caster) in [
        ("wide", wide, sun(30.0)),
        ("narrow", narrow, sun(50.0)),
        ("narrow, side light", narrow, side_sun),
    ] {
        let (plan, _) = ShadowPlan::new(&[caster], Some(&camera));
        let cascades = plan.directional.unwrap();
        let splits = cascades.split_far;
        for i in 0..SHADOW_CASCADE_COUNT {
            let start = if i == 0 {
                camera.near
            } else {
                let before = if i >= 2 { splits[i - 2] } else { 0.0 };
                splits[i - 1] - (splits[i - 1] - before) * CASCADE_BLEND
            };
            for corner in camera.slice_corners(start, splits[i]) {
                let ndc = clip(&cascades.views[i].view_projection(), corner);
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z),
                    "{name}: cascade {i} misses corner {corner} of [{start}, {}]: {ndc}",
                    splits[i]
                );
            }
        }
    }
}

#[test]
fn a_scaled_camera_fits_each_cascade_to_the_view_depths_that_select_it() {
    // `directional_shadow` picks a cascade by the Z of `camera.view`, the
    // inverse of the camera's whole world transform, scale included. So
    // the world points at each cascade's view depths, through that same
    // view, must land inside the cascade.
    let camera = Camera::new(16.0 / 9.0, 70f32.to_radians(), 0.1, 1000.0);
    let transform = Transform::from_components(
        vec3(1.0, 2.0, -3.0),
        UnitQuaternion::face_towards(&vec3(0.2, -0.3, 1.0).normalize(), &Vec3::y()),
        vec3(2.0, 2.0, 2.0),
    );
    let shadow_camera = ShadowCamera::new(&camera, &transform);
    let (plan, _) = ShadowPlan::new(&[sun(30.0)], Some(&shadow_camera));
    let cascades = plan.directional.unwrap();
    let splits = cascades.split_far;
    let inverse_view = glm::inverse(&transform.inverse_matrix());
    let tan_x = (camera.fov_x * 0.5).tan();
    let tan_y = (math::to_fov_y(camera.aspect, camera.fov_x) * 0.5).tan();
    for i in 0..SHADOW_CASCADE_COUNT {
        let start = cascade_fit_start(shadow_camera.near, &splits, i);
        for depth in [start, splits[i]] {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let view_point = vec4(x * depth * tan_x, y * depth * tan_y, depth, 1.0);
                let corner = (inverse_view * view_point).xyz();
                let ndc = clip(&cascades.views[i].view_projection(), corner);
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z),
                    "cascade {i} misses the point at view depth {depth}: {ndc}"
                );
            }
        }
    }
}

#[test]
fn a_moving_camera_moves_cascades_by_whole_texels_and_keeps_their_size() {
    let forward = vec3(0.2, -0.3, 1.0).normalize();
    let (before, _) = ShadowPlan::new(&[sun(30.0)], Some(&camera_at(Vec3::zeros(), forward)));
    let (after, _) = ShadowPlan::new(
        &[sun(30.0)],
        Some(&camera_at(vec3(0.137, 0.0, 0.291), forward)),
    );
    let (before, after) = (before.directional.unwrap(), after.directional.unwrap());
    for i in 0..SHADOW_CASCADE_COUNT {
        assert_eq!(before.texel_size[i], after.texel_size[i]);
        // A fixed world point moves across the map by a whole number of
        // texels, so the rasterized shadow does not shimmer.
        let point = vec3(1.0, 0.5, 2.0);
        let shift = (clip(&after.views[i].view_projection(), point)
            - clip(&before.views[i].view_projection(), point))
        .xy()
            * (SHADOW_CASCADE_RESOLUTION as f32 / 2.0);
        for axis in [shift.x, shift.y] {
            assert!(
                (axis - axis.round()).abs() < 1e-2,
                "cascade {i} moved by a fraction of a texel: {shift}"
            );
        }
    }
    // Turning the camera keeps each cascade's size.
    let (turned, _) = ShadowPlan::new(&[sun(30.0)], Some(&camera_at(Vec3::zeros(), Vec3::x())));
    assert_eq!(turned.directional.unwrap().texel_size, before.texel_size);
}

#[test]
fn the_nearest_spot_lights_within_their_distance_get_tiles() {
    let camera = camera_at(Vec3::zeros(), Vec3::z());
    let mut casters: Vec<Option<ShadowCaster>> = (0..10)
        .map(|i| spot(vec3(0.0, 0.0, 10.0 - i as f32), 100.0))
        .collect();
    // Past its shadow distance, so it gets no tile however near it is.
    casters.push(spot(vec3(0.0, 0.0, 0.5), 0.1));
    casters.push(None);
    let (plan, indices) = ShadowPlan::new(&casters, Some(&camera));
    assert_eq!(plan.spots.len(), MAX_SPOT_SHADOWS);
    // Nearest first: the last spot created is the nearest.
    assert_eq!(indices[9], 0);
    assert_eq!(indices[2], 7);
    assert_eq!(
        &indices[0..2],
        [-1, -1],
        "the two farthest lose their shadow"
    );
    assert_eq!(&indices[10..], [-1, -1]);
    let tiles: Vec<u32> = plan.spots.iter().map(|spot| spot.tile).collect();
    assert_eq!(tiles, (0..MAX_SPOT_SHADOWS as u32).collect::<Vec<_>>());
}

#[test]
fn tiles_fill_the_atlas_row_by_row_without_overlapping() {
    assert_eq!(tile_origin(0), (0, 0));
    assert_eq!(tile_origin(1), (SHADOW_ATLAS_TILE, 0));
    assert_eq!(tile_origin(ATLAS_COLUMNS), (0, SHADOW_ATLAS_TILE));
    for tile in 0..MAX_SPOT_SHADOWS as u32 {
        let (x, y) = tile_origin(tile);
        assert!(x + SHADOW_ATLAS_TILE <= SHADOW_ATLAS_RESOLUTION);
        assert!(y + SHADOW_ATLAS_TILE <= SHADOW_ATLAS_RESOLUTION);
    }
}

#[test]
fn a_spot_shadow_covers_its_cone_out_to_its_range() {
    let position = vec3(1.0, 4.0, 2.0);
    let view = fit_spot(
        3,
        position,
        -Vec3::y(),
        10.0,
        30f32.to_radians(),
        settings(50.0),
    );
    let edge = 29f32.to_radians();
    let in_cone = position + vec3(edge.sin(), -edge.cos(), 0.0) * 9.0;
    let ndc = clip(&view.view.view_projection(), in_cone);
    assert!(ndc.x.abs() < 1.0 && ndc.y.abs() < 1.0 && (0.0..1.0).contains(&ndc.z));
    let past_range = clip(&view.view.view_projection(), position - Vec3::y() * 11.0);
    assert!(past_range.z > 1.0);
}

#[test]
fn each_shadow_view_binds_a_full_camera_uniform_of_its_map() {
    // `shaders/mesh/shadow.wgsl` binds `CameraUniforms` of
    // `shaders/common/camera.wgsl`, so each cascade and spot tile uploads
    // a whole camera: its matrices with their inverses, its clip planes
    // and the size of the map it renders into.
    let camera = camera_at(vec3(2.0, 3.0, -5.0), vec3(0.2, -0.4, 1.0).normalize());
    let (plan, _) = ShadowPlan::new(&[sun(30.0), spot(vec3(1.0, 4.0, 2.0), 50.0)], Some(&camera));
    let cascades = plan.directional.unwrap();
    let views = cascades
        .views
        .iter()
        .map(|view| (view, SHADOW_CASCADE_RESOLUTION))
        .chain(
            plan.spots
                .iter()
                .map(|spot| (&spot.view, SHADOW_ATLAS_TILE)),
        );
    for (view, size) in views {
        let uniform = view.camera_uniform(size);
        let projection = Mat4::from(uniform.projection);
        let view_matrix = Mat4::from(uniform.view);
        assert!(
            (projection * view_matrix - view.view_projection())
                .abs()
                .max()
                < 1e-5
        );
        let identity = Mat4::identity();
        let inverse_projection = Mat4::from(uniform.inverse_projection);
        let inverse_view = Mat4::from(uniform.inverse_view);
        assert!((inverse_projection * projection - identity).abs().max() < 1e-3);
        assert!((inverse_view * view_matrix - identity).abs().max() < 1e-4);
        assert_eq!(uniform.viewport_size, [size as f32; 2]);
        // The clip planes are where the projection puts depth 0 and 1.
        let depth = |z: f32| {
            let clip = projection * vec4(0.0, 0.0, z, 1.0);
            clip.z / clip.w
        };
        assert!(depth(uniform.near_plane).abs() < 1e-4);
        assert!((depth(uniform.far_plane) - 1.0).abs() < 1e-4);
    }
}

#[test]
fn the_shadow_data_matches_the_shader_layout() {
    assert_eq!(size_of::<GpuShadowCascade>(), 80);
    assert_eq!(size_of::<GpuSpotShadow>(), 80);
    assert_eq!(std::mem::offset_of!(GpuShadowData, spots), 320);
    assert_eq!(std::mem::offset_of!(GpuShadowData, cascade_blend), 960);
    assert_eq!(
        std::mem::offset_of!(GpuShadowData, directional_normal_bias),
        964
    );
    assert_eq!(std::mem::offset_of!(GpuShadowData, atlas_tile_size), 968);
    assert_eq!(std::mem::offset_of!(GpuShadowData, atlas_texel), 972);
    assert_eq!(std::mem::offset_of!(GpuShadowData, cascade_texel), 976);
    // The shader rounds the struct up to its 16-byte alignment.
    assert_eq!(size_of::<GpuShadowData>(), 992);
}

#[test]
fn depth_bias_scales_with_the_light_setting() {
    assert_eq!(
        settings(1.0).depth_bias_state().slope_scale,
        DEPTH_BIAS_SLOPE
    );
    let none = ShadowSettings {
        depth_bias: 0.0,
        ..settings(1.0)
    };
    assert_eq!(none.depth_bias_state(), wgpu::DepthBiasState::default());
}
