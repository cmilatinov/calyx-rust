//! The CPU rules of fog of war sight, which the pass's shader repeats.

use super::*;
use crate::render::postprocess::{BlendEffect, PostProcessSettings};
use nalgebra_glm::vec2;

/// A viewer at `position` whose sight fades from 12 to 32 units out.
fn viewer(position: Vec2) -> FogOfWarViewer {
    FogOfWarViewer {
        position,
        fade_start: 12.0,
        fade_end: 32.0,
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

/// A viewer at the origin and a 2 by 2 block centred 5 units out along +X.
fn behind_a_block() -> FogOfWarSight {
    FogOfWarSight {
        viewers: vec![viewer(Vec2::zeros())],
        blockers: vec![block(vec2(5.0, 0.0), vec2(2.0, 2.0))],
        border: Default::default(),
    }
}

#[test]
fn a_blocker_is_seen_and_the_ground_behind_it_is_not() {
    let sight = behind_a_block();
    assert_eq!(sight.visibility(vec2(3.9, 0.0)), 1.0, "before the block");
    assert_eq!(sight.visibility(vec2(4.5, 0.0)), 1.0, "on the block");
    assert_eq!(sight.visibility(vec2(5.99, 0.0)), 1.0, "its far side");
    assert_eq!(sight.visibility(vec2(6.01, 0.0)), 0.0, "just behind it");
    assert_eq!(sight.visibility(vec2(11.0, 0.5)), 0.0, "deep in its shadow");
    assert_eq!(sight.visibility(vec2(11.0, 4.0)), 1.0, "beside the shadow");
}

#[test]
fn a_point_on_a_blockers_far_side_is_on_the_blocker() {
    // A wall's face turned from the viewer rebuilds from depth to points on
    // the footprint's edge, give or take the depth's rounding: they are all
    // the blocker's, and seen, wherever the line to them crosses it.
    let sight = behind_a_block();
    for step in 0..=200 {
        let z = -0.99 + step as f32 * 0.0099;
        for x in [6.0 - 1e-3, 6.0, 6.0 + 1e-3, 6.0 + 0.004] {
            assert_eq!(sight.visibility(vec2(x, z)), 1.0, "({x}, {z})");
        }
    }
    // Further out it is ground behind the block, in its shadow.
    assert_eq!(sight.visibility(vec2(6.0 + 0.006, 0.0)), 0.0);
}

#[test]
fn a_shadow_side_is_the_line_from_the_viewer_past_the_corner() {
    // The block's corner (4, 1) bounds its shadow on the +Z side: the side
    // runs along the line through it, at a quarter of the distance in z.
    let sight = behind_a_block();
    for x in [8.0, 12.0, 20.0] {
        let edge = x / 4.0;
        assert_eq!(sight.visibility(vec2(x, edge - 0.01)), 0.0, "{x}");
        assert!(sight.visibility(vec2(x, edge + 0.01)) > 0.0, "{x}");
    }
}

#[test]
fn sight_carries_through_touching_blockers_but_not_across_a_gap() {
    let mut sight = behind_a_block();
    // A block touching the first one's far side, and one past a gap.
    sight.blockers.push(block(vec2(7.02, 0.0), vec2(2.0, 2.0)));
    sight.blockers.push(block(vec2(10.0, 0.0), vec2(2.0, 2.0)));
    assert_eq!(
        sight.visibility(vec2(7.5, 0.0)),
        1.0,
        "on the touching block"
    );
    assert_eq!(sight.visibility(vec2(8.1, 0.0)), 0.0, "past the run");
    assert_eq!(
        sight.visibility(vec2(10.0, 0.0)),
        0.0,
        "on the block past it"
    );
}

#[test]
fn a_rotated_blocker_casts_the_shadow_of_its_footprint() {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let sight = FogOfWarSight {
        viewers: vec![viewer(Vec2::zeros())],
        blockers: vec![FogOfWarBlocker {
            center: vec2(5.0, 0.0),
            half_axes: [vec2(diagonal, diagonal), vec2(-diagonal, diagonal)],
        }],
        border: Default::default(),
    };
    // A diamond from x = 5 - √2 to 5 + √2 along the line to it.
    assert_eq!(sight.visibility(vec2(6.4, 0.0)), 1.0);
    assert_eq!(sight.visibility(vec2(6.45, 0.0)), 0.0);
    // Lines of sight that pass its slanted sides, though they cross its
    // axis-aligned bounds, are clear.
    assert_eq!(sight.visibility(vec2(8.0, 2.8)), 1.0);
    assert_eq!(sight.visibility(vec2(8.0, 2.0)), 0.0);
}

#[test]
fn a_viewer_inside_a_blocker_sees_out_to_its_side_only() {
    let sight = FogOfWarSight {
        viewers: vec![viewer(Vec2::zeros())],
        blockers: vec![block(Vec2::zeros(), vec2(2.0, 2.0))],
        border: Default::default(),
    };
    assert_eq!(sight.visibility(vec2(0.9, 0.0)), 1.0);
    assert_eq!(sight.visibility(vec2(1.1, 0.0)), 0.0);
}

#[test]
fn the_range_fade_falls_monotonically_along_its_exponential() {
    let viewer = viewer(Vec2::zeros());
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
        ..viewer
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
fn a_viewers_weight_scales_what_it_lends() {
    let mut sight = behind_a_block();
    sight.viewers[0].weight = 0.25;
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 0.25);
    assert_eq!(sight.visibility(vec2(8.0, 0.0)), 0.0);
    sight.viewers[0].weight = 0.0;
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 0.0);
    assert_eq!(FogOfWarViewer::default().weight, 1.0);
}

#[test]
fn the_side_sees_a_point_as_well_as_its_best_viewer() {
    let mut sight = behind_a_block();
    let point = vec2(8.0, 0.0);
    assert_eq!(
        sight.visibility(point),
        0.0,
        "behind the near viewer's cover"
    );
    let far = viewer(vec2(8.0, -20.0));
    let partial = far.sight(20.0);
    assert!(partial > 0.0 && partial < 1.0, "{partial}");
    sight.viewers.push(far);
    assert_eq!(sight.visibility(point), partial);
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 1.0);
    assert_eq!(FogOfWarSight::default().visibility(point), 0.0);
}

#[test]
fn a_pixel_averages_its_samples_across_a_shadow_side() {
    let sight = behind_a_block();
    let (dx, dy) = (vec2(0.1, 0.0), vec2(0.0, 0.1));
    // Away from every edge a pixel is what its centre is.
    assert_eq!(sight.pixel_visibility(vec2(2.0, 3.0), dx, dy), 1.0);
    assert_eq!(sight.pixel_visibility(vec2(10.0, 0.0), dx, dy), 0.0);
    // A pixel on the shadow's side is partly seen, and steps between its
    // centre's values as it slides across the side.
    let across = (0..=20)
        .map(|step| sight.pixel_visibility(vec2(8.0, 1.9 + step as f32 * 0.01), dx, dy))
        .collect::<Vec<_>>();
    assert_eq!(across[0], 0.0);
    assert_eq!(across[20], 1.0);
    assert!(across.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!(across.iter().any(|&seen| seen > 0.0 && seen < 1.0));
}

#[test]
fn the_sight_is_runtime_state_that_is_never_saved() {
    let fog = ComponentFogOfWar {
        sight: Some(behind_a_block()),
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
    let sight = |x| FogOfWarSight {
        viewers: vec![viewer(vec2(x, 0.0))],
        blockers: vec![block(vec2(x, 3.0), vec2(1.0, 1.0)); 3],
        border: Default::default(),
    };
    let mut value = ComponentFogOfWar::default();
    value.copy_in_place(&ComponentFogOfWar {
        sight: Some(sight(3.0)),
        ..Default::default()
    });
    let storage = value.sight.as_ref().unwrap().blockers.as_ptr();
    let source = ComponentFogOfWar {
        opacity: 0.5,
        sight: Some(sight(4.0)),
        ..Default::default()
    };
    value.copy_in_place(&source);
    assert_eq!(value, source);
    assert_eq!(value.sight.as_ref().unwrap().blockers.as_ptr(), storage);
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

/// A 20 by 20 area walled round, 1 unit thick, with a finger of wall
/// reaching down into it from the north, 3 wide, round a slot of outside
/// 1 wide: two stretches running close together, and a cove either side of
/// the finger.
fn walled() -> FogOfWarBorder {
    FogOfWarBorder {
        inner: vec![
            vec2(-10.0, -10.0),
            vec2(10.0, -10.0),
            vec2(10.0, 10.0),
            vec2(1.5, 10.0),
            vec2(1.5, 0.0),
            vec2(-1.5, 0.0),
            vec2(-1.5, 10.0),
            vec2(-10.0, 10.0),
        ],
        outer: vec![
            vec2(-11.0, -11.0),
            vec2(11.0, -11.0),
            vec2(11.0, 11.0),
            vec2(0.5, 11.0),
            vec2(0.5, 1.0),
            vec2(-0.5, 1.0),
            vec2(-0.5, 11.0),
            vec2(-11.0, 11.0),
        ],
    }
}

/// A viewer in the east cove of [`walled`], beside the finger.
fn in_the_cove() -> FogOfWarSight {
    FogOfWarSight {
        viewers: vec![viewer(vec2(5.0, 5.0))],
        blockers: Vec::new(),
        border: walled(),
    }
}

#[test]
fn ground_just_outside_the_border_is_never_seen() {
    let sight = in_the_cove();
    assert_eq!(sight.visibility(vec2(9.9, 5.0)), 1.0, "before the wall");
    assert_eq!(sight.visibility(vec2(10.5, 5.0)), 1.0, "on the wall");
    assert_eq!(sight.visibility(vec2(10.999, 5.0)), 1.0, "its outer face");
    assert_eq!(sight.visibility(vec2(11.01, 5.0)), 0.0, "just past it");
    assert_eq!(sight.visibility(vec2(12.0, 5.0)), 0.0, "outside");
    assert_eq!(
        sight.visibility(vec2(5.0, 11.02)),
        0.0,
        "past the north wall"
    );
    // The faces of the wall in sight are the wall's, give or take the
    // depth's rounding, on either side of them.
    for z in [1.0f32, 3.3, 7.7, 9.0] {
        for x in [10.0 - 0.004, 10.0, 10.0 + 0.004] {
            assert_eq!(sight.visibility(vec2(x, z)), 1.0, "({x}, {z})");
        }
    }
    // A viewer outside the border sees nothing, even next to it.
    let outside = FogOfWarSight {
        viewers: vec![viewer(vec2(12.0, 5.0))],
        ..in_the_cove()
    };
    assert_eq!(outside.visibility(vec2(12.0, 5.5)), 0.0);
    assert_eq!(outside.visibility(vec2(9.0, 5.0)), 0.0);
}

#[test]
fn two_close_stretches_hide_the_outside_between_them_and_all_past_it() {
    // The finger's two walls run 1 apart, round a slot of outside.
    let sight = in_the_cove();
    assert_eq!(
        sight.visibility(vec2(1.6, 5.0)),
        1.0,
        "before the near wall"
    );
    assert_eq!(sight.visibility(vec2(1.0, 5.0)), 1.0, "on the near wall");
    for x in [0.49, 0.2, 0.0, -0.2, -0.49] {
        assert_eq!(sight.visibility(vec2(x, 5.0)), 0.0, "in the slot at {x}");
    }
    assert_eq!(sight.visibility(vec2(-1.0, 5.0)), 0.0, "on the far wall");
    assert_eq!(sight.visibility(vec2(-1.6, 5.0)), 0.0, "past both");
    assert_eq!(sight.visibility(vec2(-6.0, 6.0)), 0.0, "in the far cove");
    // Along the slot, no line slips through between the walls: every point
    // across the finger and past it is out of sight.
    for step in 1..100 {
        let z = step as f32 * 0.1;
        for x in [-0.6f32, -1.4, -1.6, -3.0] {
            assert_eq!(sight.visibility(vec2(x, z)), 0.0, "({x}, {z})");
        }
    }
}

#[test]
fn a_cove_s_point_casts_its_shadow_from_the_wall_s_inner_corner() {
    // The finger's tip, (1.5, 0) on its inner face, bounds the shadow it
    // casts over the far cove: a line from the viewer passing just inside
    // it cuts through the wall's corner and is stopped, however little
    // wall it crosses, and one passing just outside it is clear.
    let sight = in_the_cove();
    let viewer = sight.viewers[0].position;
    let corner = vec2(1.5, 0.0);
    let along = (corner - viewer).normalize();
    let across = vec2(-along.y, along.x);
    for distance in [2.0f32, 4.0, 5.5] {
        let past = corner + along * distance;
        // `across` points away from the finger, towards -z.
        assert_eq!(
            sight.visibility(past + across * 0.01),
            1.0,
            "clear of the corner, {distance} past it"
        );
        assert_eq!(
            sight.visibility(past - across * 0.01),
            0.0,
            "through the corner, {distance} past it"
        );
    }
    // The finger's face towards the viewer is in sight, and so is the top
    // of its tip.
    assert_eq!(sight.visibility(vec2(1.501, 0.5)), 1.0);
    assert_eq!(sight.visibility(vec2(0.0, 0.5)), 1.0);
}

#[test]
fn the_border_s_reach_is_where_a_ray_leaves_the_wall() {
    let border = walled();
    let from = vec2(5.0, 5.0);
    // Out east: the wall's outer face.
    assert!((border.reach(from, vec2(1.0, 0.0), 30.0) - 6.0).abs() < 1e-4);
    // West across the finger: over its near wall, to the slot of outside.
    assert!((border.reach(from, vec2(-1.0, 0.0), 30.0) - 4.5).abs() < 1e-4);
    // South-west past its tip, through the wall's corner: to where the ray
    // comes back into the area.
    let through = border.reach(from, vec2(-1.0, -1.0).normalize(), 30.0);
    assert!((through - 5.0 * 2f32.sqrt()).abs() < 1e-4, "{through}");
    // Short of the wall, the limit.
    assert_eq!(border.reach(from, vec2(0.0, -1.0), 4.0), 4.0);
    assert_eq!(border.reach(vec2(0.0, 5.0), vec2(1.0, 0.0), 30.0), 0.0);
    assert_eq!(
        FogOfWarBorder::default().reach(from, vec2(1.0, 0.0), 30.0),
        30.0
    );
}

#[test]
fn blockers_inside_the_border_still_cast_their_shadows() {
    let mut sight = in_the_cove();
    sight.blockers.push(block(vec2(7.0, 2.0), vec2(1.0, 1.0)));
    assert_eq!(sight.visibility(vec2(7.0, 2.0)), 1.0, "on the block");
    assert_eq!(sight.visibility(vec2(8.0, 0.5)), 0.0, "behind it");
    assert_eq!(sight.visibility(vec2(8.0, 4.0)), 1.0, "beside it");
}
