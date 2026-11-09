use super::*;
use crate::vision::{GroundRect, Viewer};
use nalgebra_glm::{vec2, Vec2};

fn viewer(id: u128, position: Vec2) -> Viewer {
    Viewer {
        id: Uuid::from_u128(id),
        position,
        range: 20.0,
    }
}

fn vision(viewers: Vec<Viewer>, cover: Vec<GroundRect>) -> TeamVision {
    TeamVision {
        viewers,
        cover,
        concealed: Vec::new(),
    }
}

/// A block of cover across +X, 3 to 5 m out and 4 m wide, in front of a
/// tank at the origin.
fn block() -> TeamVision {
    vision(
        vec![viewer(1, Vec2::zeros())],
        vec![GroundRect::axis_aligned(vec2(4.0, 0.0), vec2(2.0, 4.0))],
    )
}

#[test]
fn the_cover_is_seen_up_to_its_far_side_and_its_shadow_is_fully_dark() {
    let sight = side_sight(&block());
    // The ground before the block and the block itself are in full view.
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 1.0);
    assert_eq!(sight.visibility(vec2(4.99, 0.0)), 1.0);
    // Just past the block's far side the shadow starts, fully dark.
    assert_eq!(sight.visibility(vec2(5.01, 0.0)), 0.0);
    assert_eq!(sight.visibility(vec2(9.0, 1.0)), 0.0);
    // Beside the shadow, the same distance out is still seen.
    assert_eq!(sight.visibility(vec2(0.0, 5.01)), 1.0);
}

#[test]
fn a_shadow_side_is_hard() {
    let sight = side_sight(&block());
    // The shadow's upper side runs from the tank through the block's corner
    // at (3, 2); 8 m out, step across it a centimetre at a time.
    let side = 2.0f32.atan2(3.0);
    let along = vec2(side.cos(), side.sin()) * 8.0;
    let across = vec2(-side.sin(), side.cos());
    let values = (0..=200)
        .map(|i| sight.visibility(along + across * ((i as f32 - 100.0) * 0.01)))
        .collect::<Vec<_>>();
    assert_eq!(values[0], 0.0, "inside the shadow");
    assert!(values[200] > 0.9, "outside it: {}", values[200]);
    let between = values
        .iter()
        .filter(|&&v| v > 0.0 && v < values[200] - 1e-4)
        .count();
    assert_eq!(between, 0, "no soft band across the side: {values:?}");
}

#[test]
fn the_range_fade_falls_monotonically_along_an_exponential() {
    let range = 30.0;
    assert_eq!(range_fade(0.0, range), 1.0);
    assert_eq!(range_fade(range - RANGE_FADE_INSIDE, range), 1.0);
    assert_eq!(range_fade(range + RANGE_FADE_OUTSIDE, range), 0.0);
    assert_eq!(range_fade(range + RANGE_FADE_OUTSIDE + 5.0, range), 0.0);
    let width = RANGE_FADE_INSIDE + RANGE_FADE_OUTSIDE;
    let samples = (0..=100)
        .map(|i| range_fade(range - RANGE_FADE_INSIDE + width * i as f32 / 100.0, range))
        .collect::<Vec<_>>();
    assert!(samples.windows(2).all(|pair| pair[1] < pair[0]));
    // Exponential: it darkens slowly at first and faster towards the end,
    // so every step darkens more than the one before.
    let steps = samples
        .windows(2)
        .map(|pair| pair[0] - pair[1])
        .collect::<Vec<_>>();
    assert!(steps.windows(2).all(|pair| pair[1] > pair[0]));
    let k = RANGE_FADE_CURVE;
    assert!((samples[50] - (1.0 - ((k * 0.5).exp() - 1.0) / (k.exp() - 1.0))).abs() < 1e-5);
}

#[test]
fn the_renderer_fades_sight_as_range_fade_does() {
    let sight = side_sight(&vision(vec![viewer(1, Vec2::zeros())], Vec::new()));
    for i in 0..=80 {
        let distance = i as f32 * 0.5;
        assert!((sight.visibility(vec2(0.0, distance)) - range_fade(distance, 20.0)).abs() < 1e-5);
    }
}

#[test]
fn several_tanks_light_the_ground_as_the_one_that_sees_it_best() {
    // A second tank beside the block sees into the first one's shadow.
    let mut both = block();
    both.viewers.push(viewer(2, vec2(8.0, 7.0)));
    let first = side_sight(&block());
    let sight = side_sight(&both);
    let point = vec2(8.0, 0.0);
    assert_eq!(first.visibility(point), 0.0);
    let second = sight.viewers[1].sight((point - sight.viewers[1].position).norm());
    assert!(second > 0.0, "{second}");
    assert_eq!(sight.visibility(point), second);
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 1.0);
}

#[test]
fn only_cover_within_reach_of_the_side_is_handed_over() {
    let mut side = block();
    // Past the end of the tank's range fade, with room to spare.
    let far = 20.0 + RANGE_FADE_OUTSIDE + 3.0;
    side.cover
        .push(GroundRect::axis_aligned(vec2(far, 0.0), vec2(1.0, 1.0)));
    side.cover
        .push(GroundRect::axis_aligned(vec2(0.0, -40.0), vec2(1.0, 1.0)));
    let sight = side_sight(&side);
    assert_eq!(
        sight
            .blockers
            .iter()
            .map(|blocker| blocker.center)
            .collect::<Vec<_>>(),
        [vec2(4.0, 0.0)]
    );
}

#[test]
fn the_shadow_follows_a_moving_tank_without_lag() {
    // Moving the tank moves its shadow sides at once: no per-ray easing
    // trails them.
    let mut fog = FogSight::default();
    let mut side = block();
    fog.update(&side, 0.0);
    let point = vec2(9.0, 7.0);
    assert!(fog.sight().visibility(point) > 0.0);
    side.viewers[0].position = vec2(0.0, -2.0);
    fog.update(&side, 0.016);
    assert_eq!(fog.sight().viewers[0].position, vec2(0.0, -2.0));
    assert_eq!(fog.sight().visibility(point), 0.0);
    assert_eq!(fog.sight(), &side_sight(&side));
}

#[test]
fn a_tank_joining_the_side_fades_its_sight_in_and_one_leaving_fades_it_out() {
    let first = viewer(1, Vec2::zeros());
    let second = viewer(2, vec2(60.0, 0.0));
    let mut fog = FogSight::default();
    let both = vision(vec![first, second], Vec::new());
    let alone = vision(vec![first], Vec::new());
    // The first update shows the side's sight in full.
    fog.update(&alone, 0.0);
    assert_eq!(fog.sight().viewers.len(), 1);
    assert_eq!(fog.sight().viewers[0].weight, 1.0);

    let tick = FADE_TIME / 3.0;
    fog.update(&both, tick);
    let joined = fog.sight().viewers[1];
    assert_eq!(joined.position, second.position);
    assert!((joined.weight - 1.0 / 3.0).abs() < 1e-5);
    assert_eq!(fog.sight().viewers[0].weight, 1.0);
    assert!((fog.sight().visibility(second.position) - 1.0 / 3.0).abs() < 1e-5);
    fog.update(&both, tick);
    fog.update(&both, tick);
    assert!((fog.sight().viewers[1].weight - 1.0).abs() < 1e-5);

    // A pause lets no fog time pass.
    fog.update(&alone, 0.0);
    assert!((fog.sight().viewers[1].weight - 1.0).abs() < 1e-5);

    // The second tank dies: its sight fades out where it last stood, then
    // goes.
    fog.update(&alone, tick);
    fog.update(&alone, tick);
    assert_eq!(fog.sight().viewers.len(), 2);
    assert_eq!(fog.sight().viewers[1].position, second.position);
    assert!((fog.sight().viewers[1].weight - 1.0 / 3.0).abs() < 1e-5);
    fog.update(&alone, tick);
    assert_eq!(fog.sight().viewers.len(), 1);
}

#[test]
fn the_minimaps_rays_stop_where_the_fogs_lines_of_sight_do() {
    let sight = side_sight(&block());
    let rays = sight_rays(&sight, 8);
    assert_eq!(rays.len(), 1);
    // Along +X the ray crosses the block and stops at its far side, where
    // the fog's shadow starts; the others run to the end of the fade.
    assert!((rays[0].through[0] - 5.0).abs() < 1e-4);
    assert!(sight.visibility(vec2(4.99, 0.0)) > 0.0);
    assert_eq!(sight.visibility(vec2(5.01, 0.0)), 0.0);
    let fade_end = 20.0 + RANGE_FADE_OUTSIDE;
    assert!((rays[0].through[2] - fade_end).abs() < 1e-4);
}

#[test]
fn no_seeing_tank_darkens_everything() {
    let mut fog = FogSight::default();
    fog.update(&TeamVision::default(), 0.016);
    assert!(fog.sight().viewers.is_empty());
    assert_eq!(fog.sight().visibility(Vec2::zeros()), 0.0);
}
