use super::*;
use crate::component::{
    ComponentAmbientLight, ComponentDirectionalLight, ComponentPointLight, ComponentSpotLight,
};
use crate::test_support::test_scene;
use egui::Color32;
use nalgebra_glm::{rotation, translation, vec3};

/// The lights as the GPU reads them.
fn gpu(lights: Vec<Light>) -> Vec<GpuLight> {
    lights.into_iter().map(GpuLight::from).collect()
}

#[test]
fn collect_point_lights_skips_disabled_lights_until_re_enabled() {
    let mut scene = test_scene();
    let enabled = scene.create(None, None);
    scene.set_world_transform(enabled, translation(&vec3(1.0, 2.0, 3.0)));
    scene.add_component(
        enabled,
        ComponentPointLight {
            radius: 7.5,
            color: Color32::RED,
            intensity: 0.25,
        },
    );

    let disabled = scene.create(None, None);
    scene.add_component(
        disabled,
        ComponentPointLight {
            radius: 99.0,
            color: Color32::BLUE,
            ..Default::default()
        },
    );
    assert!(scene.set_component_enabled::<ComponentPointLight>(disabled, false));

    let lights = gpu(LightManager::collect_point_lights(&scene));

    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].position, [1.0, 2.0, 3.0]);
    assert_eq!(lights[0].range, 7.5);
    assert_eq!(lights[0].color, [1.0, 0.0, 0.0]);
    assert_eq!(lights[0].intensity, 0.25);

    assert!(scene.set_component_enabled::<ComponentPointLight>(disabled, true));
    let lights = gpu(LightManager::collect_point_lights(&scene));
    assert_eq!(lights.len(), 2);
    assert!(lights.iter().any(|light| light.range == 99.0));
}

#[test]
fn collect_directional_lights_skips_disabled_lights_until_re_enabled() {
    let mut scene = test_scene();
    let enabled = scene.create(None, None);
    scene.add_component(
        enabled,
        ComponentDirectionalLight {
            color: Color32::GREEN,
            intensity: 0.5,
            ..Default::default()
        },
    );

    let disabled = scene.create(None, None);
    scene.add_component(
        disabled,
        ComponentDirectionalLight {
            color: Color32::BLUE,
            ..Default::default()
        },
    );
    assert!(scene.set_component_enabled::<ComponentDirectionalLight>(disabled, false));

    let lights = gpu(LightManager::collect_directional_lights(&scene));

    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].direction, [0.0, 0.0, 1.0]);
    assert_eq!(lights[0].color, [0.0, 1.0, 0.0]);
    assert_eq!(lights[0].intensity, 0.5);

    assert!(scene.set_component_enabled::<ComponentDirectionalLight>(disabled, true));
    assert_eq!(LightManager::collect_directional_lights(&scene).len(), 2);
}

#[test]
fn gpu_light_matches_the_shader_layout() {
    assert_eq!(size_of::<GpuLight>(), 64);
    assert_eq!(std::mem::offset_of!(GpuLight, range), 12);
    assert_eq!(std::mem::offset_of!(GpuLight, direction), 16);
    assert_eq!(std::mem::offset_of!(GpuLight, cos_inner), 28);
    assert_eq!(std::mem::offset_of!(GpuLight, color), 32);
    assert_eq!(std::mem::offset_of!(GpuLight, intensity), 44);
    assert_eq!(std::mem::offset_of!(GpuLight, cos_outer), 48);
    assert_eq!(std::mem::offset_of!(GpuLight, shadow_index), 52);
    // The light array starts at its 16-byte alignment, after the counts.
    assert_eq!(size_of::<LightStorageHeader>(), 16);
}

#[test]
fn a_light_zeroes_the_fields_its_kind_does_not_use_and_has_no_shadow() {
    let directional = GpuLight::from(Light::Directional {
        direction: [0.0, -1.0, 0.0],
        color: [1.0, 0.5, 0.25],
        intensity: 2.0,
        shadow: None,
    });
    assert_eq!(
        directional,
        GpuLight {
            direction: [0.0, -1.0, 0.0],
            color: [1.0, 0.5, 0.25],
            intensity: 2.0,
            shadow_index: -1,
            ..Default::default()
        }
    );

    let point = GpuLight::from(Light::Point {
        position: [1.0, 2.0, 3.0],
        range: 4.0,
        color: [1.0, 1.0, 1.0],
        intensity: 0.5,
    });
    assert_eq!(
        point,
        GpuLight {
            position: [1.0, 2.0, 3.0],
            range: 4.0,
            color: [1.0, 1.0, 1.0],
            intensity: 0.5,
            shadow_index: -1,
            ..Default::default()
        }
    );
}

#[test]
fn a_mixed_set_of_lights_uploads_grouped_by_kind_with_their_counts() {
    let mut scene = test_scene();
    // Created interleaved, and each identified by its intensity.
    let add_spot = |scene: &mut Scene, intensity: f32| {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentSpotLight {
                intensity,
                ..Default::default()
            },
        );
    };
    let add_point = |scene: &mut Scene, intensity: f32| {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentPointLight {
                intensity,
                ..Default::default()
            },
        );
    };
    let add_directional = |scene: &mut Scene, intensity: f32| {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentDirectionalLight {
                intensity,
                ..Default::default()
            },
        );
    };
    add_spot(&mut scene, 1.0);
    add_point(&mut scene, 2.0);
    add_directional(&mut scene, 3.0);
    add_spot(&mut scene, 4.0);
    add_point(&mut scene, 5.0);
    add_spot(&mut scene, 6.0);

    let packed = PackedLights::new(LightManager::collect_lights(&scene), None);

    assert_eq!(
        packed.header,
        LightStorageHeader {
            directional_count: 1,
            point_count: 2,
            spot_count: 3,
            _padding: 0,
        }
    );
    assert_eq!(packed.lights.len(), 6);
    let intensities = |range: std::ops::Range<usize>| {
        let mut intensities: Vec<f32> = packed.lights[range]
            .iter()
            .map(|light| light.intensity)
            .collect();
        intensities.sort_by(f32::total_cmp);
        intensities
    };
    assert_eq!(intensities(0..1), [3.0], "directional lights come first");
    assert_eq!(intensities(1..3), [2.0, 5.0], "then the point lights");
    assert_eq!(intensities(3..6), [1.0, 4.0, 6.0], "then the spot lights");
    // Only a spot light carries cone cosines.
    assert!(packed.lights[..3]
        .iter()
        .all(|light| light.cos_outer == 0.0));
    assert!(packed.lights[3..].iter().all(|light| light.cos_outer > 0.0));
    assert!(packed.lights.iter().all(|light| light.shadow_index == -1));
}

#[test]
fn no_lights_pack_to_zero_counts() {
    assert_eq!(PackedLights::new(Vec::new(), None), PackedLights::default());
}

fn shadow_camera() -> ShadowCamera {
    ShadowCamera::new(
        &crate::render::Camera::default(),
        &crate::math::Transform::from_xyz(0.0, 0.0, -5.0),
    )
}

#[test]
fn shadowed_lights_carry_their_shadow_index_and_others_none() {
    let mut scene = test_scene();
    let add_directional = |scene: &mut Scene, intensity: f32, cast_shadows: bool| {
        let object = scene.create(None, None);
        scene.add_component(
            object,
            ComponentDirectionalLight {
                intensity,
                cast_shadows,
                ..Default::default()
            },
        );
        object
    };
    let add_spot = |scene: &mut Scene, intensity: f32, z: f32, cast_shadows: bool| {
        let object = scene.create(None, None);
        scene.set_world_transform(object, translation(&vec3(0.0, 0.0, z)));
        scene.add_component(
            object,
            ComponentSpotLight {
                intensity,
                cast_shadows,
                ..Default::default()
            },
        );
        object
    };
    add_directional(&mut scene, 1.0, false);
    let sun = add_directional(&mut scene, 2.0, true);
    add_directional(&mut scene, 3.0, true);
    add_spot(&mut scene, 4.0, 20.0, true);
    add_spot(&mut scene, 5.0, -4.0, true);
    add_spot(&mut scene, 6.0, 0.0, false);

    let shadow_index = |packed: &PackedLights, intensity: f32| {
        packed
            .lights
            .iter()
            .find(|light| light.intensity == intensity)
            .unwrap()
            .shadow_index
    };
    let camera = shadow_camera();
    let packed = PackedLights::new(LightManager::collect_lights(&scene), Some(&camera));
    assert_eq!(shadow_index(&packed, 1.0), -1, "it casts no shadow");
    assert_eq!(shadow_index(&packed, 2.0), 0, "the first shadowed sun");
    assert_eq!(shadow_index(&packed, 3.0), -1, "only one sun gets cascades");
    assert_eq!(shadow_index(&packed, 5.0), 0, "the nearest spot light");
    assert_eq!(shadow_index(&packed, 4.0), 1);
    assert_eq!(shadow_index(&packed, 6.0), -1);
    assert!(packed.shadows.directional.is_some());
    assert_eq!(packed.shadows.spots.len(), 2);

    // Disabling the shadowed sun frees its cascades for the next one, and
    // enabling it takes them back.
    assert!(scene.set_component_enabled::<ComponentDirectionalLight>(sun, false));
    let packed = PackedLights::new(LightManager::collect_lights(&scene), Some(&camera));
    assert_eq!(shadow_index(&packed, 3.0), 0);
    assert!(scene.set_component_enabled::<ComponentDirectionalLight>(sun, true));
    let packed = PackedLights::new(LightManager::collect_lights(&scene), Some(&camera));
    assert_eq!(shadow_index(&packed, 2.0), 0);
    assert_eq!(shadow_index(&packed, 3.0), -1);

    // Without a camera, as with shadows switched off, nothing has one.
    let packed = PackedLights::new(LightManager::collect_lights(&scene), None);
    assert!(packed.lights.iter().all(|light| light.shadow_index == -1));
    assert!(packed.shadows.is_empty());
}

#[test]
fn a_disabled_spot_light_gives_up_its_tile_until_re_enabled() {
    let mut scene = test_scene();
    let spot = scene.create(None, None);
    scene.add_component(
        spot,
        ComponentSpotLight {
            cast_shadows: true,
            ..Default::default()
        },
    );
    let camera = shadow_camera();
    let plan = |scene: &Scene| {
        PackedLights::new(LightManager::collect_lights(scene), Some(&camera)).shadows
    };
    assert_eq!(plan(&scene).spots.len(), 1);
    assert!(scene.set_component_enabled::<ComponentSpotLight>(spot, false));
    assert!(plan(&scene).is_empty());
    assert!(scene.set_component_enabled::<ComponentSpotLight>(spot, true));
    assert_eq!(plan(&scene).spots.len(), 1);
}

#[test]
fn collect_spot_lights_uses_world_position_direction_and_cone_cosines() {
    let mut scene = test_scene();
    let enabled = scene.create(None, None);
    // A quarter turn about Y takes +Z to +X.
    scene.set_world_transform(
        enabled,
        translation(&vec3(1.0, 2.0, 3.0)) * rotation(90f32.to_radians(), &vec3(0.0, 1.0, 0.0)),
    );
    scene.add_component(
        enabled,
        ComponentSpotLight {
            color: Color32::RED,
            intensity: 2.0,
            range: 6.0,
            inner_angle: 60.0,
            outer_angle: 80.0,
            ..Default::default()
        },
    );

    let disabled = scene.create(None, None);
    scene.add_component(
        disabled,
        ComponentSpotLight {
            range: 99.0,
            ..Default::default()
        },
    );
    assert!(scene.set_component_enabled::<ComponentSpotLight>(disabled, false));

    let lights = gpu(LightManager::collect_spot_lights(&scene));

    assert_eq!(lights.len(), 1);
    let light = lights[0];
    assert_eq!(light.position, [1.0, 2.0, 3.0]);
    let direction = Vec3::from(light.direction);
    assert!(
        (direction - vec3(1.0, 0.0, 0.0)).norm() < 1e-5,
        "{direction}"
    );
    assert_eq!(light.range, 6.0);
    assert_eq!(light.color, [1.0, 0.0, 0.0]);
    assert_eq!(light.intensity, 2.0);
    assert!((light.cos_inner - 0.5).abs() < 1e-5, "{}", light.cos_inner);
    assert!((light.cos_outer - 80f32.to_radians().cos()).abs() < 1e-6);

    assert!(scene.set_component_enabled::<ComponentSpotLight>(disabled, true));
    let lights = gpu(LightManager::collect_spot_lights(&scene));
    assert_eq!(lights.len(), 2);
    let default = lights.iter().find(|light| light.range == 99.0).unwrap();
    assert_eq!(default.direction, [0.0, 0.0, 1.0]);
}

#[test]
fn collect_spot_lights_keeps_the_inner_cone_within_the_outer_one() {
    let mut scene = test_scene();
    let object = scene.create(None, None);
    scene.add_component(
        object,
        ComponentSpotLight {
            inner_angle: 45.0,
            outer_angle: 30.0,
            ..Default::default()
        },
    );

    let light = gpu(LightManager::collect_spot_lights(&scene))[0];

    assert_eq!(light.cos_inner, light.cos_outer);
    assert!((light.cos_outer - 30f32.to_radians().cos()).abs() < 1e-6);
}

#[test]
fn collect_ambient_light_sums_enabled_lights_until_re_enabled() {
    let mut scene = test_scene();
    let first = scene.create(None, None);
    scene.add_component(
        first,
        ComponentAmbientLight {
            color: Color32::RED,
            intensity: 0.25,
        },
    );
    let second = scene.create(None, None);
    scene.add_component(
        second,
        ComponentAmbientLight {
            color: Color32::BLUE,
            intensity: 0.5,
        },
    );
    let disabled = scene.create(None, None);
    scene.add_component(
        disabled,
        ComponentAmbientLight {
            color: Color32::GREEN,
            intensity: 1.0,
        },
    );
    assert!(scene.set_component_enabled::<ComponentAmbientLight>(disabled, false));

    assert_eq!(
        LightManager::collect_ambient_light(&scene),
        [0.25, 0.0, 0.5, 1.0]
    );

    assert!(scene.set_component_enabled::<ComponentAmbientLight>(disabled, true));
    assert_eq!(
        LightManager::collect_ambient_light(&scene),
        [0.25, 1.0, 0.5, 1.0]
    );
}

/// `light_distance_attenuation` from `lights.wgsl`, evaluated on the GPU
/// at each `(distance, range)` pair.
fn gpu_attenuation(queries: &[[f32; 2]]) -> Vec<f32> {
    const KERNEL: &str = r#"
//#include "shaders/common/lights.wgsl"

@group(0) @binding(0) var<storage, read> queries: array<vec2f>;
@group(0) @binding(1) var<storage, read_write> values: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= arrayLength(&queries) {
        return;
    }
    let query = queries[gid.x];
    values[gid.x] = light_distance_attenuation(query.x, query.y);
}

// A second entry point, so `main` binds only the resources it uses and not
// the light storage `lights.wgsl` declares in group 2.
@compute @workgroup_size(1)
fn unused() {}
"#;
    crate::render::test_kernel::run_kernel(KERNEL, &[bytemuck::cast_slice(queries)], queries.len())
}

#[test]
fn point_and_spot_falloff_is_inverse_square_windowed_to_zero_at_range() {
    let ranges = [0.5, 1.0, 6.0, 12.0, 100.0];
    // 64 even steps from the light to its range, then past it.
    let steps: Vec<f32> = (0..=80).map(|i| i as f32 / 64.0).collect();
    let queries: Vec<[f32; 2]> = ranges
        .iter()
        .flat_map(|&range| steps.iter().map(move |&t| [t * range, range]))
        .collect();
    let values = gpu_attenuation(&queries);

    for (range, values) in ranges.iter().zip(values.chunks(steps.len())) {
        assert_eq!(values[0], 1.0, "range {range}: full intensity at the light");
        for (i, pair) in values[..=64].windows(2).enumerate() {
            assert!(
                pair[1] < pair[0],
                "range {range}: decreases from step {i} ({}) to {} ({})",
                pair[0],
                i + 1,
                pair[1]
            );
        }
        for (i, value) in values[64..].iter().enumerate() {
            assert_eq!(
                *value,
                0.0,
                "range {range}: zero at {} ranges",
                1.0 + i as f32 / 64.0
            );
        }
    }

    // Well inside the range the falloff is 1 / (d² + 1); halfway out the
    // window (1 - (d/r)^4)^2 = (15/16)^2 dims it.
    let values = gpu_attenuation(&[[1.0, 100.0], [3.0, 100.0], [2.0, 4.0]]);
    let expected = [0.5, 0.1, 0.878_906_25 / 5.0];
    for (value, expected) in values.iter().zip(expected) {
        assert!(
            (value - expected).abs() < 1e-4,
            "falloff {value}, expected {expected}"
        );
    }
}
