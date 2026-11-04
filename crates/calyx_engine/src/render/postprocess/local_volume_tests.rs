//! Local post-process volumes: blend factors, ordering and per-effect
//! blending over the global volume's settings.

use super::*;
use crate::component::{
    ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom, ComponentColorGrading,
    ComponentDepthOfField, ComponentFog, ComponentGlobalPostProcessVolume,
    ComponentLocalPostProcessVolume, ComponentOutline, ComponentTonemap, ComponentVignette,
    FogMode, LocalVolumeShape, TonemappingPreset,
};
use crate::math::{compose_transform, Transform};
use crate::scene::{GameObject, Scene};
use crate::test_support::test_registries;
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use uuid::Uuid;

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{what}: expected {expected}, got {actual}"
    );
}

fn box_volume(
    half_extents: Vec3,
    blend_distance: f32,
    weight: f32,
) -> ComponentLocalPostProcessVolume {
    ComponentLocalPostProcessVolume {
        shape: LocalVolumeShape::Box { half_extents },
        blend_distance,
        weight,
        ..Default::default()
    }
}

#[test]
fn a_box_volume_blends_by_weight_inside_and_fades_over_the_blend_distance() {
    let volume = box_volume(vec3(1.0, 1.0, 1.0), 2.0, 0.8);
    let at = |x: f32| volume.blend_factor(&Transform::default(), &vec3(x, 0.0, 0.0));
    assert_close(at(0.0), 0.8, "inside");
    assert_close(at(1.0), 0.8, "at the edge");
    assert_close(at(2.0), 0.4, "halfway through the blend distance");
    assert_close(at(3.0), 0.0, "at the blend distance");
    assert_close(at(5.0), 0.0, "beyond it");
    // Past a corner the distance is to the corner, not to either face.
    let corner = volume.blend_factor(&Transform::default(), &vec3(2.0, 2.0, 1.0));
    assert_close(corner, 0.8 * (1.0 - 2f32.sqrt() / 2.0), "past a corner");
}

#[test]
fn a_box_volume_rotates_and_scales_with_its_game_object() {
    let volume = box_volume(vec3(1.0, 1.0, 1.0), 2.0, 1.0);
    // A quarter turn about Y turns the local X axis, scaled by 2, onto world Z.
    let transform = Transform::from_components(
        vec3(10.0, 0.0, 0.0),
        UnitQuaternion::from_axis_angle(&Vec3::y_axis(), 90f32.to_radians()),
        vec3(2.0, 1.0, 1.0),
    );
    let at = |x: f32, z: f32| volume.blend_factor(&transform, &vec3(x, 0.0, z));
    assert_close(at(10.0, 1.9), 1.0, "inside the scaled axis");
    assert_close(at(10.0, -2.0), 1.0, "at the scaled edge");
    assert_close(at(10.0, 3.0), 0.5, "one unit past the scaled edge");
    assert_close(at(11.5, 0.0), 0.75, "half a unit past the unscaled edge");
    assert_close(at(12.0, 0.0), 0.5, "the world X side is not the scaled one");
    assert_close(at(13.5, 0.0), 0.0, "beyond the blend distance");
}

#[test]
fn a_sphere_volume_blends_by_distance_from_its_surface() {
    let volume = ComponentLocalPostProcessVolume {
        shape: LocalVolumeShape::Sphere { radius: 1.0 },
        blend_distance: 2.0,
        weight: 0.5,
        ..Default::default()
    };
    // Its radius scales with the largest axis of the object's scale.
    let transform = Transform::from_components(
        vec3(0.0, 5.0, 0.0),
        UnitQuaternion::identity(),
        vec3(1.0, 2.0, 1.0),
    );
    let at = |y: f32| volume.blend_factor(&transform, &vec3(0.0, y, 0.0));
    assert_close(at(5.0), 0.5, "at the centre");
    assert_close(at(6.5), 0.5, "inside");
    assert_close(at(7.0), 0.5, "at the edge");
    assert_close(at(8.0), 0.25, "halfway through the blend distance");
    assert_close(at(9.0), 0.0, "at the blend distance");
    assert_close(at(20.0), 0.0, "beyond it");
}

#[test]
fn a_volume_without_a_blend_distance_switches_at_its_surface() {
    let volume = box_volume(vec3(1.0, 1.0, 1.0), 0.0, 1.0);
    let at = |x: f32| volume.blend_factor(&Transform::default(), &vec3(x, 0.0, 0.0));
    assert_close(at(1.0), 1.0, "at the edge");
    assert_close(at(1.001), 0.0, "just outside");
}

/// A global volume at the origin's object with `bloom`, if any, and tone
/// mapping at `exposure_ev`.
fn spawn_global(scene: &mut Scene, exposure_ev: f32, bloom: Option<ComponentBloom>) -> GameObject {
    let game_object = scene.create(None, None);
    scene.add_component(game_object, ComponentGlobalPostProcessVolume::default());
    scene.add_component(
        game_object,
        ComponentTonemap {
            exposure_ev,
            ..Default::default()
        },
    );
    if let Some(bloom) = bloom {
        scene.add_component(game_object, bloom);
    }
    game_object
}

/// A local box volume centred at `position` with half extents of 1 and a
/// blend distance of 2.
fn spawn_local(scene: &mut Scene, position: Vec3, weight: f32, priority: i32) -> GameObject {
    let game_object = scene.create(None, None);
    scene.set_transform(
        game_object,
        &compose_transform(&position, &UnitQuaternion::identity(), &vec3(1.0, 1.0, 1.0)),
    );
    scene.add_component(
        game_object,
        ComponentLocalPostProcessVolume {
            priority,
            ..box_volume(vec3(1.0, 1.0, 1.0), 2.0, weight)
        },
    );
    game_object
}

fn bloom(intensity: f32, radius: u32) -> ComponentBloom {
    ComponentBloom {
        intensity,
        radius,
        ..Default::default()
    }
}

#[test]
fn a_local_volume_blends_its_effects_over_the_global_settings_by_camera_position() {
    let registries = test_registries();
    let mut scene = registries.scene();
    spawn_global(&mut scene, 0.0, Some(bloom(0.2, 6)));
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(local, bloom(1.0, 6));

    let resolve = |x: f32| PostProcessSettings::resolve(&scene, &vec3(x, 0.0, 0.0));
    let global = PostProcessSettings::from_game_object(
        &scene,
        PostProcessSettings::selected_volume(&scene).unwrap(),
    );
    assert_close(resolve(0.0).bloom.unwrap().intensity, 1.0, "inside");
    assert_close(resolve(2.0).bloom.unwrap().intensity, 0.6, "halfway out");
    assert_eq!(resolve(3.0), global, "a factor of zero contributes nothing");
    assert_eq!(resolve(50.0), global);
}

#[test]
fn without_a_global_volume_local_volumes_blend_over_the_defaults() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let local = spawn_local(&mut scene, Vec3::zeros(), 0.5, 0);
    scene.add_component(
        local,
        ComponentTonemap {
            exposure_ev: 2.0,
            ..Default::default()
        },
    );
    let settings = PostProcessSettings::resolve(&scene, &Vec3::zeros());
    assert_close(settings.tonemapping.exposure_ev, 1.0, "half of 2 EV over 0");
    assert_eq!(
        PostProcessSettings::resolve(&scene, &vec3(10.0, 0.0, 0.0)),
        PostProcessSettings::default()
    );
}

#[test]
fn overlapping_local_volumes_apply_in_ascending_priority() {
    let registries = test_registries();
    let mut scene = registries.scene();
    spawn_global(&mut scene, 0.0, None);
    // The higher priority is spawned first, so creation order would apply
    // it first.
    let high = spawn_local(&mut scene, Vec3::zeros(), 0.5, 5);
    let low = spawn_local(&mut scene, Vec3::zeros(), 0.5, 1);
    for (game_object, exposure_ev) in [(high, 4.0), (low, 2.0)] {
        scene.add_component(
            game_object,
            ComponentTonemap {
                exposure_ev,
                ..Default::default()
            },
        );
    }
    assert_eq!(
        PostProcessSettings::local_volumes(&scene, &Vec3::zeros()),
        vec![(low, 0.5), (high, 0.5)]
    );
    // 0 EV, then half way to 2 (1 EV), then half way to 4.
    let settings = PostProcessSettings::resolve(&scene, &Vec3::zeros());
    assert_close(
        settings.tonemapping.exposure_ev,
        2.5,
        "the higher applies last",
    );
}

#[test]
fn tied_local_volumes_apply_in_object_id_order_and_report_the_tie() {
    let ids = [Uuid::from_u128(3), Uuid::from_u128(1), Uuid::from_u128(2)];
    let order = order_local_volumes([(0, ids[0], 'a'), (0, ids[1], 'b'), (-1, ids[2], 'c')]);
    assert_eq!(order.ordered, vec!['c', 'b', 'a']);
    assert_eq!(order.tie, Some(0));

    let order = order_local_volumes([(2, ids[0], 'a'), (1, ids[1], 'b')]);
    assert_eq!(order.ordered, vec!['b', 'a']);
    assert_eq!(order.tie, None);
}

#[test]
fn an_effect_the_local_volume_lacks_or_disables_inherits_the_value_below() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let global = spawn_global(&mut scene, 1.0, Some(bloom(0.2, 6)));
    let vignette = ComponentVignette {
        intensity: 0.3,
        ..Default::default()
    };
    scene.add_component(global, vignette);
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(local, bloom(1.0, 6));
    assert!(scene.set_component_enabled::<ComponentBloom>(local, false));

    let settings = PostProcessSettings::resolve(&scene, &Vec3::zeros());
    assert_eq!(settings.bloom, Some(bloom(0.2, 6)), "a disabled effect");
    assert_eq!(settings.vignette, Some(vignette), "an absent effect");
    assert_close(settings.tonemapping.exposure_ev, 1.0, "absent tone mapping");
}

#[test]
fn a_disabled_local_volume_takes_no_part_in_the_blend() {
    let registries = test_registries();
    let mut scene = registries.scene();
    spawn_global(&mut scene, 0.0, None);
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(local, bloom(1.0, 6));
    assert!(scene.set_component_enabled::<ComponentLocalPostProcessVolume>(local, false));

    assert!(PostProcessSettings::local_volumes(&scene, &Vec3::zeros()).is_empty());
    assert_eq!(
        PostProcessSettings::resolve(&scene, &Vec3::zeros()).bloom,
        None
    );
}

#[test]
fn an_effect_off_below_fades_in_from_its_neutral_values() {
    let below = PostProcessSettings::default();
    let effects = PostProcessEffects {
        ambient_occlusion: Some(ComponentAmbientOcclusion {
            intensity: 2.0,
            radius: 0.8,
            ..Default::default()
        }),
        depth_of_field: Some(ComponentDepthOfField {
            max_blur: 8.0,
            focus_distance: 4.0,
            ..Default::default()
        }),
        bloom: Some(ComponentBloom {
            threshold: 2.0,
            ..bloom(1.0, 4)
        }),
        color_grading: Some(ComponentColorGrading {
            saturation: 0.0,
            contrast: 1.5,
            ..Default::default()
        }),
        vignette: Some(ComponentVignette {
            intensity: 0.8,
            ..Default::default()
        }),
        ..Default::default()
    };
    let settings = below.blend(&effects, 0.25);

    let ssao = settings.ambient_occlusion.unwrap();
    assert_close(ssao.intensity, 0.5, "SSAO intensity from 0");
    assert_close(ssao.radius, 0.8, "SSAO keeps the volume's radius");
    let dof = settings.depth_of_field.unwrap();
    assert_close(dof.max_blur, 2.0, "depth of field blur from 0");
    assert_close(dof.focus_distance, 4.0, "and keeps the volume's focus");
    let bloom = settings.bloom.unwrap();
    assert_close(bloom.intensity, 0.25, "bloom intensity from 0");
    assert_close(bloom.threshold, 2.0, "bloom keeps the volume's threshold");
    assert_eq!(bloom.radius, 4);
    let grading = settings.color_grading.unwrap();
    assert_close(grading.saturation, 0.75, "saturation from neutral 1");
    assert_close(grading.contrast, 1.125, "contrast from neutral 1");
    assert_close(grading.temperature, 0.0, "temperature stays neutral");
    assert_close(
        settings.vignette.unwrap().intensity,
        0.2,
        "vignette intensity from 0",
    );
}

#[test]
fn fxaa_has_no_neutral_value_and_switches_on_at_half() {
    let effects = PostProcessEffects {
        anti_aliasing: Some(ComponentAntiAliasing::default()),
        ..Default::default()
    };
    let below = PostProcessSettings::default();
    assert_eq!(below.blend(&effects, 0.49).anti_aliasing, None);
    assert_eq!(
        below.blend(&effects, 0.5).anti_aliasing,
        Some(ComponentAntiAliasing::default())
    );
}

#[test]
fn integer_and_enum_fields_switch_at_half_while_floats_interpolate() {
    let below = PostProcessSettings {
        bloom: Some(bloom(0.0, 2)),
        ambient_occlusion: Some(ComponentAmbientOcclusion {
            sample_count: 8,
            ..Default::default()
        }),
        ..Default::default()
    };
    let effects = PostProcessEffects {
        bloom: Some(bloom(1.0, 6)),
        ambient_occlusion: Some(ComponentAmbientOcclusion {
            sample_count: 32,
            ..Default::default()
        }),
        tonemapping: Some(ComponentTonemap::default()),
        ..Default::default()
    };
    let before = below.blend(&effects, 0.49);
    assert_eq!(before.bloom.unwrap().radius, 2);
    assert_close(
        before.bloom.unwrap().intensity,
        0.49,
        "intensity interpolates",
    );
    assert_eq!(before.ambient_occlusion.unwrap().sample_count, 8);
    let after = below.blend(&effects, 0.5);
    assert_eq!(after.bloom.unwrap().radius, 6);
    assert_eq!(after.ambient_occlusion.unwrap().sample_count, 32);
    assert_eq!(after.tonemapping.preset, ComponentTonemap::default().preset);
}

#[test]
fn the_tone_mapping_preset_switches_at_half_while_exposure_interpolates() {
    let registries = test_registries();
    let mut scene = registries.scene();
    // A global ACES volume at 0 EV, and a local neutral one at 2 EV whose
    // factor is 1 inside, 0.5 at x = 2 and 0.45 at x = 2.1.
    spawn_global(&mut scene, 0.0, None);
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(
        local,
        ComponentTonemap {
            preset: TonemappingPreset::Neutral,
            exposure_ev: 2.0,
        },
    );
    let resolve = |x: f32| PostProcessSettings::resolve(&scene, &vec3(x, 0.0, 0.0)).tonemapping;

    assert_eq!(resolve(0.0).preset, TonemappingPreset::Neutral, "inside");
    assert_eq!(resolve(2.0).preset, TonemappingPreset::Neutral, "at half");
    assert_close(resolve(2.0).exposure_ev, 1.0, "exposure at half");
    assert_eq!(resolve(2.1).preset, TonemappingPreset::Aces, "below half");
    assert_close(resolve(2.1).exposure_ev, 0.9, "exposure below half");
    assert_eq!(resolve(10.0), ComponentTonemap::default(), "outside");
}

#[test]
fn colours_interpolate_in_linear_space() {
    let below = PostProcessSettings {
        vignette: Some(ComponentVignette {
            color: Color32::BLACK,
            ..Default::default()
        }),
        ..Default::default()
    };
    let effects = PostProcessEffects {
        vignette: Some(ComponentVignette {
            color: Color32::WHITE,
            ..Default::default()
        }),
        ..Default::default()
    };
    let color = below.blend(&effects, 0.5).vignette.unwrap().color;
    // Linear 0.5 encodes to sRGB 188; a gamma-space blend would give 128.
    assert!((187..=189).contains(&color.r()), "{color:?}");
    assert_eq!(color.r(), color.g());
    assert_eq!(color.a(), 255);
    assert_eq!(
        below.blend(&effects, 1.0).vignette.unwrap().color,
        Color32::WHITE
    );
}

#[test]
fn the_scene_renderer_resolves_settings_at_the_camera_it_draws_with() {
    let assets = crate::test_support::TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut scene = context.registries.scene();
    spawn_global(&mut scene, 0.0, None);
    let local = spawn_local(&mut scene, vec3(20.0, 0.0, 0.0), 1.0, 0);
    scene.add_component(local, bloom(1.0, 6));
    let mut renderer = crate::render::SceneRenderer::new(&context, Default::default(), (4, 4))
        .expect("scene renderer should load its assets");
    let camera = crate::render::Camera::new(1.0, 60f32.to_radians(), 0.1, 100.0);
    let mut frame = |position: Vec3| {
        let transform =
            Transform::from_components(position, UnitQuaternion::identity(), vec3(1.0, 1.0, 1.0));
        renderer.render_scene_base(&camera, &transform, &scene, None, None);
        renderer.finalize_scene();
        renderer.post_process_settings().bloom
    };
    assert_eq!(frame(Vec3::zeros()), None, "outside the volume");
    assert_eq!(
        frame(vec3(20.0, 0.0, 0.0)),
        Some(bloom(1.0, 6)),
        "inside it"
    );
}

#[test]
fn a_local_volume_and_its_effects_save_and_load_with_the_scene() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let local = spawn_local(&mut scene, vec3(1.0, 2.0, 3.0), 0.7, 3);
    let volume = ComponentLocalPostProcessVolume {
        shape: LocalVolumeShape::Sphere { radius: 4.0 },
        blend_distance: 1.5,
        weight: 0.7,
        priority: 3,
        ..Default::default()
    };
    scene.write_component(local, |v: &mut ComponentLocalPostProcessVolume| {
        *v = volume.clone()
    });
    scene.add_component(local, bloom(0.9, 5));
    scene.set_component_enabled::<ComponentLocalPostProcessVolume>(local, false);

    let saved = serde_json::to_string(&scene).unwrap();
    let loaded: Scene = (
        &registries,
        serde_json::from_str::<crate::scene::SceneData>(&saved).unwrap(),
    )
        .into();
    let object = loaded
        .find(scene.uuid(local))
        .expect("the volume's object loads");
    assert_eq!(
        loaded.read_component(object, |v: &ComponentLocalPostProcessVolume| v.clone()),
        Some(volume)
    );
    assert_eq!(
        loaded.read_component(object, |b: &ComponentBloom| *b),
        Some(bloom(0.9, 5))
    );
    assert!(!loaded.is_component_enabled::<ComponentLocalPostProcessVolume>(object));

    // A partial component loads with the defaults.
    let partial: ComponentLocalPostProcessVolume =
        serde_json::from_str(r#"{"priority": 2}"#).unwrap();
    assert_eq!(
        partial,
        ComponentLocalPostProcessVolume {
            priority: 2,
            ..Default::default()
        }
    );
}

#[test]
fn a_point_inside_a_scaled_box_without_a_blend_distance_gets_the_full_weight() {
    // The round trip through the object's transform must not push an inside
    // point a rounding error outside the box.
    let volume = box_volume(vec3(5.0, 5.0, 5.0), 0.0, 0.7);
    let transform = Transform::from_components(
        Vec3::zeros(),
        UnitQuaternion::identity(),
        vec3(3.0, 1.0, 1.0),
    );
    for x in [0.1, 0.7, 1.3, -4.9, 14.9] {
        let factor = volume.blend_factor(&transform, &vec3(x, 0.2, -0.3));
        assert_eq!(factor, 0.7, "at x = {x}");
    }
}

fn fog(mode: FogMode, color: Color32, start: f32, end: f32) -> ComponentFog {
    ComponentFog {
        mode,
        color,
        start,
        end,
        ..Default::default()
    }
}

#[test]
fn fog_blends_between_a_global_and_a_local_volume_by_camera_position() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let global = spawn_global(&mut scene, 0.0, None);
    scene.add_component(
        global,
        ComponentFog {
            intensity: 1.0,
            density: 0.02,
            ..fog(FogMode::Linear, Color32::BLACK, 0.0, 100.0)
        },
    );
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(
        local,
        ComponentFog {
            intensity: 3.0,
            density: 0.06,
            ..fog(FogMode::Exponential, Color32::WHITE, 20.0, 300.0)
        },
    );

    let resolve = |x: f32| PostProcessSettings::resolve(&scene, &vec3(x, 0.0, 0.0)).fog;
    let inside = resolve(0.0).expect("fog inside the volume");
    assert_eq!(inside.mode, FogMode::Exponential, "inside");
    assert_eq!(inside.color, Color32::WHITE);

    // Halfway through the blend distance.
    let halfway = resolve(2.0).expect("fog halfway out");
    assert_close(halfway.start, 10.0, "start interpolates");
    assert_close(halfway.end, 200.0, "end interpolates");
    assert_close(halfway.density, 0.04, "density interpolates");
    assert_close(halfway.intensity, 2.0, "intensity interpolates");
    assert_close(halfway.opacity, 1.0, "opacity stays full");
    assert_eq!(
        halfway.mode,
        FogMode::Exponential,
        "the mode switches at 0.5"
    );
    // Linear 0.5 encodes to sRGB 188; a gamma-space blend would give 128.
    assert!(
        (187..=189).contains(&halfway.color.r()),
        "the colour blends in linear space: {:?}",
        halfway.color
    );
    let short_of_half = resolve(2.02).expect("fog just past halfway out");
    assert_eq!(
        short_of_half.mode,
        FogMode::Linear,
        "below 0.5 the mode stays"
    );

    assert_eq!(
        resolve(3.0),
        Some(fog(FogMode::Linear, Color32::BLACK, 0.0, 100.0))
            .map(|f| ComponentFog { density: 0.02, ..f }),
        "outside the blend distance the global fog applies"
    );
}

#[test]
fn fog_off_below_fades_in_from_no_opacity() {
    let below = PostProcessSettings::default();
    let effects = PostProcessEffects {
        fog: Some(ComponentFog {
            opacity: 0.8,
            ..fog(FogMode::Linear, Color32::WHITE, 5.0, 50.0)
        }),
        ..Default::default()
    };

    let settings = below.blend(&effects, 0.25);

    let fog = settings.fog.expect("fog fades in");
    assert_close(fog.opacity, 0.2, "opacity from 0");
    assert_close(fog.start, 5.0, "keeps the volume's start");
    assert_close(fog.end, 50.0, "keeps the volume's end");
    assert_eq!(fog.color, Color32::WHITE, "keeps the volume's colour");
    assert_eq!(below.blend(&effects, 0.0).fog.map(|f| f.opacity), Some(0.0));
}

#[test]
fn outlines_blend_between_a_global_and_a_local_volume_and_fade_in_from_no_opacity() {
    let registries = test_registries();
    let mut scene = registries.scene();
    let global = spawn_global(&mut scene, 0.0, None);
    let thin = ComponentOutline {
        color: Color32::BLACK,
        width: 1.0,
        depth_threshold: 0.02,
        normal_threshold: 60.0,
        opacity: 1.0,
    };
    scene.add_component(global, thin);
    let local = spawn_local(&mut scene, Vec3::zeros(), 1.0, 0);
    scene.add_component(
        local,
        ComponentOutline {
            color: Color32::WHITE,
            width: 3.0,
            depth_threshold: 0.04,
            normal_threshold: 20.0,
            opacity: 0.5,
        },
    );

    let resolve = |x: f32| PostProcessSettings::resolve(&scene, &vec3(x, 0.0, 0.0)).outline;
    assert_eq!(resolve(0.0).map(|o| o.width), Some(3.0), "inside");
    // Halfway through the blend distance.
    let halfway = resolve(2.0).expect("outlines halfway out");
    assert_close(halfway.width, 2.0, "width interpolates");
    assert_close(
        halfway.depth_threshold,
        0.03,
        "depth threshold interpolates",
    );
    assert_close(
        halfway.normal_threshold,
        40.0,
        "normal threshold interpolates",
    );
    assert_close(halfway.opacity, 0.75, "opacity interpolates");
    // Linear 0.5 encodes to sRGB 188; a gamma-space blend would give 128.
    assert!(
        (187..=189).contains(&halfway.color.r()),
        "the ink blends in linear space: {:?}",
        halfway.color
    );
    assert_eq!(
        resolve(3.0),
        Some(thin),
        "outside, the global outlines apply"
    );

    // Off below, a volume's outlines fade in from no ink.
    let effects = PostProcessEffects {
        outline: Some(thin),
        ..Default::default()
    };
    let below = PostProcessSettings::default();
    let faded = below
        .blend(&effects, 0.25)
        .outline
        .expect("outlines fade in");
    assert_close(faded.opacity, 0.25, "opacity from 0");
    assert_close(faded.width, 1.0, "keeps the volume's width");
    assert_eq!(faded.color, Color32::BLACK, "keeps the volume's ink");
    assert_eq!(
        below.blend(&effects, 0.0).outline.map(|o| o.opacity),
        Some(0.0)
    );
}
