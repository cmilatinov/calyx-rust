//! Type-erased operations keep the renderer's typed blend rules, including
//! discrete thresholds and the outline effect's variable-length styles.

use super::*;
use crate::render::postprocess::BlendEffect;
use egui::Color32;
use std::fmt::Debug;

#[test]
#[should_panic(expected = "post-process effects must have the same concrete type")]
fn derived_copy_rejects_a_different_effect_type() {
    ComponentBloom::default().copy_from(&ComponentFog::default());
}

#[test]
#[should_panic(expected = "post-process effects must have the same concrete type")]
fn derived_blend_rejects_a_different_effect_type() {
    ComponentBloom::default().blend_into(&ComponentFog::default(), 0.5);
}

fn check_effect<T: PostProcessEffect + BlendEffect + PartialEq + Debug>(from: T, to: T) {
    let mut value = from.clone();
    let erased: &mut dyn PostProcessEffect = &mut value;
    erased.copy_from(&to);
    assert_eq!(value, to);

    for factor in [-1.0_f32, 0.0, 0.25, 0.499, 0.5, 0.75, 1.0, 2.0] {
        let mut value = from.clone();
        let erased: &mut dyn PostProcessEffect = &mut value;
        erased.blend_into(&to, factor);
        assert_eq!(value, from.blend(&to, factor.clamp(0.0, 1.0)));
    }

    let mut value = to.clone();
    let has_neutral = (&mut value as &mut dyn PostProcessEffect).reset_neutral();
    match T::neutral(&to) {
        Some(neutral) => {
            assert!(has_neutral);
            assert_eq!(value, neutral);
            (&mut value as &mut dyn PostProcessEffect).blend_into(&to, 0.25);
            assert_eq!(value, neutral.blend(&to, 0.25));
        }
        None => {
            assert!(!has_neutral);
            assert_eq!(value, to, "a discrete effect has no neutral reset");
        }
    }
}

#[test]
fn erased_effect_operations_preserve_all_builtin_blend_rules() {
    check_effect(
        ComponentTonemap::default(),
        ComponentTonemap {
            preset: TonemappingPreset::Neutral,
            exposure_ev: 2.0,
        },
    );
    check_effect(
        ComponentAmbientOcclusion::default(),
        ComponentAmbientOcclusion {
            intensity: 2.5,
            sample_count: 32,
            radius: 1.0,
            bias: 0.05,
        },
    );
    check_effect(
        ComponentDepthOfField::default(),
        ComponentDepthOfField {
            focus_distance: 20.0,
            focus_range: 4.0,
            max_blur: 16.0,
        },
    );
    check_effect(
        ComponentBloom::default(),
        ComponentBloom {
            intensity: 3.0,
            threshold: 2.0,
            knee: 0.2,
            radius: 9,
        },
    );
    check_effect(
        ComponentColorGrading::default(),
        ComponentColorGrading {
            contrast: 1.5,
            saturation: 0.5,
            temperature: 0.75,
            tint: -0.25,
        },
    );
    check_effect(
        ComponentVignette::default(),
        ComponentVignette {
            intensity: 0.75,
            color: Color32::from_rgb(80, 30, 10),
            smoothness: 0.4,
        },
    );
    check_effect(
        ComponentAntiAliasing::default(),
        ComponentAntiAliasing {
            edge_threshold: 0.05,
            subpixel: 0.25,
            quality: FxaaQuality::High,
        },
    );
    check_effect(
        ComponentFog::default(),
        ComponentFog {
            mode: FogMode::Exponential,
            color: Color32::from_rgb(240, 120, 40),
            intensity: 3.0,
            start: 10.0,
            end: 50.0,
            density: 0.05,
            opacity: 0.4,
        },
    );
    check_effect(
        ComponentFogOfWar::default(),
        ComponentFogOfWar {
            color: Color32::from_rgb(10, 20, 30),
            opacity: 0.3,
            sight: Some(FogOfWarSight {
                viewers: vec![FogOfWarViewer {
                    weight: 0.5,
                    ..Default::default()
                }],
                blockers: vec![FogOfWarBlocker::default(); 2],
            }),
        },
    );
    let outline = |count| ComponentOutline {
        styles: (0..count)
            .map(|index| OutlineStyle {
                color: Color32::from_rgb(20 + index * 30, 10, 50),
                width: 1.0 + index as f32,
                opacity: 0.5 + index as f32 * 0.1,
                ..Default::default()
            })
            .collect(),
        depth_threshold: 0.01 + count as f32 * 0.001,
        silhouette_threshold: 0.04 + count as f32 * 0.005,
        normal_threshold: 20.0 + count as f32 * 10.0,
    };
    check_effect(outline(1), outline(3));
    check_effect(outline(3), outline(1));
    check_effect(outline(0), outline(3));
}

#[test]
fn outline_reuses_its_style_storage_across_copy_blend_and_neutral_reset() {
    let full = ComponentOutline {
        styles: vec![OutlineStyle::default(); 8],
        ..Default::default()
    };
    let short = ComponentOutline {
        styles: vec![OutlineStyle::default(); 2],
        ..Default::default()
    };
    let mut value = full.clone();
    let storage = value.styles.as_ptr();
    let capacity = value.styles.capacity();
    for _ in 0..10 {
        PostProcessEffect::copy_from(&mut value, &short);
        assert_eq!(value.styles.len(), 2);
        assert!(value.reset_neutral());
        value.blend_into(&full, 0.25);
        assert_eq!(value.styles.len(), 8);
        value.blend_into(&short, 0.5);
        assert_eq!(value.styles.as_ptr(), storage);
        assert_eq!(value.styles.capacity(), capacity);
    }
}
