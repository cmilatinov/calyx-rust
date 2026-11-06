//! The CPU rules of fog of war sight, which the pass's shader repeats.

use super::*;
use crate::render::postprocess::{BlendEffect, PostProcessSettings};
use nalgebra_glm::vec2;
use std::f32::consts::TAU;

/// A viewer at the origin whose 720 rays reach `far`, except those whose
/// angle lies within `half_width` radians of `towards`, which reach `near`.
fn viewer(towards: f32, half_width: f32, near: f32, far: f32) -> FogOfWarViewer {
    FogOfWarViewer {
        position: Vec2::zeros(),
        fade_start: 12.0,
        fade_end: 32.0,
        fade_curve: 3.0,
        reach: (0..720)
            .map(|index| {
                let angle = index as f32 * TAU / 720.0;
                let off = (angle - towards + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0;
                if off.abs() <= half_width {
                    near
                } else {
                    far
                }
            })
            .collect(),
        shadow_jump: 0.3,
    }
}

fn at(angle: f32, distance: f32) -> Vec2 {
    vec2(angle.cos(), angle.sin()) * distance
}

#[test]
fn a_point_just_inside_the_reach_is_seen_and_one_just_past_it_is_not() {
    let viewer = viewer(0.0, 0.2, 5.0, 40.0);
    assert_eq!(viewer.visibility(at(0.0, 4.99)), 1.0);
    assert_eq!(viewer.visibility(at(0.0, 5.01)), 0.0);
    // Off the shadow, sight goes on to the fade.
    assert_eq!(viewer.visibility(at(1.0, 11.0)), 1.0);
}

#[test]
fn a_shadow_side_is_a_hard_radial_edge_halfway_between_two_rays() {
    // The shadow covers rays 0 to 10 of 720; its side lies between rays 10
    // and 11, halfway at 10.5 ray spacings.
    let spacing = TAU / 720.0;
    let viewer = viewer(0.0, 10.2 * spacing, 5.0, 40.0);
    for distance in [6.0, 10.0, 11.5] {
        assert_eq!(
            viewer.visibility(at(10.45 * spacing, distance)),
            0.0,
            "{distance} m out, just inside the shadow"
        );
        assert_eq!(
            viewer.visibility(at(10.55 * spacing, distance)),
            1.0,
            "{distance} m out, just outside it"
        );
    }
}

#[test]
fn neighbouring_reaches_that_differ_little_interpolate() {
    let mut viewer = viewer(0.0, 0.0, 5.0, 5.0);
    viewer.reach[1] = 5.2;
    let spacing = TAU / 720.0;
    assert!((viewer.reach_at(at(0.5 * spacing, 1.0)) - 5.1).abs() < 1e-4);
    assert!((viewer.reach_at(at(0.25 * spacing, 1.0)) - 5.05).abs() < 1e-4);
}

#[test]
fn the_range_fade_falls_monotonically_along_its_exponential() {
    let viewer = viewer(0.0, 0.0, 40.0, 40.0);
    assert_eq!(viewer.range_fade(0.0), 1.0);
    assert_eq!(viewer.range_fade(12.0), 1.0);
    assert_eq!(viewer.range_fade(32.0), 0.0);
    assert_eq!(viewer.range_fade(50.0), 0.0);
    let k = 3.0f32;
    for step in 0..=20 {
        let t = step as f32 / 20.0;
        let expected = 1.0 - ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        assert!((viewer.range_fade(12.0 + 20.0 * t) - expected).abs() < 1e-5);
    }
    let samples: Vec<f32> = (0..=400)
        .map(|i| viewer.range_fade(10.0 + i as f32 * 0.06))
        .collect();
    assert!(samples.windows(2).all(|pair| pair[1] <= pair[0]));
    // Brighter than a linear fade throughout: it darkens slowly at first.
    assert!(viewer.range_fade(22.0) > 0.5);

    let linear = FogOfWarViewer {
        fade_curve: 0.0,
        ..viewer.clone()
    };
    assert!((linear.range_fade(22.0) - 0.5).abs() < 1e-6);
    let sharp = FogOfWarViewer {
        fade_end: 12.0,
        ..viewer
    };
    assert_eq!(sharp.range_fade(11.9), 1.0);
    assert_eq!(sharp.range_fade(12.0), 0.0);
}

#[test]
fn the_side_sees_a_point_as_well_as_its_best_viewer() {
    let near = viewer(0.0, 0.3, 3.0, 40.0);
    let mut far = viewer(0.0, 0.0, 40.0, 40.0);
    far.position = vec2(-20.0, 0.0);
    let point = vec2(8.0, 0.0);
    assert_eq!(
        near.visibility(point),
        0.0,
        "behind the near viewer's cover"
    );
    let partial = far.visibility(point);
    assert!(partial > 0.0 && partial < 1.0, "{partial}");
    let sight = FogOfWarSight {
        viewers: vec![near.clone(), far],
    };
    assert_eq!(sight.visibility(point), partial);
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 1.0);
    assert_eq!(FogOfWarSight::default().visibility(point), 0.0);
    let open = FogOfWarViewer {
        reach: Vec::new(),
        ..near
    };
    assert_eq!(open.visibility(point), 1.0, "no rays cut nothing short");
}

#[test]
fn the_sight_is_runtime_state_that_is_never_saved() {
    let fog = ComponentFogOfWar {
        sight: Some(FogOfWarSight {
            viewers: vec![viewer(0.0, 0.1, 3.0, 9.0)],
        }),
        ..Default::default()
    };
    let saved = serde_json::to_value(&fog).expect("saves");
    assert_eq!(
        saved,
        serde_json::json!({ "color": [0, 0, 0, 255], "opacity": 0.64f32 })
    );
    let loaded: ComponentFogOfWar = serde_json::from_value(saved).expect("loads");
    assert_eq!(loaded, ComponentFogOfWar::default());
}

#[test]
fn settings_carry_the_sight_and_reuse_its_storage() {
    let sight = |near| FogOfWarSight {
        viewers: vec![viewer(0.0, 0.1, near, 9.0)],
    };
    let mut value = ComponentFogOfWar::default();
    value.copy_in_place(&ComponentFogOfWar {
        sight: Some(sight(3.0)),
        ..Default::default()
    });
    let storage = value.sight.as_ref().unwrap().viewers[0].reach.as_ptr();
    let source = ComponentFogOfWar {
        opacity: 0.5,
        sight: Some(sight(4.0)),
        ..Default::default()
    };
    value.copy_in_place(&source);
    assert_eq!(value, source);
    assert_eq!(
        value.sight.as_ref().unwrap().viewers[0].reach.as_ptr(),
        storage
    );
    let settings = PostProcessSettings::default().with(source.clone());
    assert_eq!(settings.get::<ComponentFogOfWar>(), Some(&source));
}

#[test]
fn a_volume_without_a_sight_keeps_the_one_below() {
    let below = ComponentFogOfWar {
        sight: Some(FogOfWarSight::default()),
        ..Default::default()
    };
    let above = ComponentFogOfWar {
        opacity: 0.2,
        ..Default::default()
    };
    let blended = below.blend(&above, 1.0);
    assert_eq!(blended.sight, below.sight);
    assert!((blended.opacity - 0.2).abs() < 1e-6);
    assert_eq!(ComponentFogOfWar::neutral(&above).unwrap().opacity, 0.0);
}
