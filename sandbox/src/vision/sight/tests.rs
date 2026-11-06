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
    TeamVision { viewers, cover }
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
    let fog = &sight.viewers[0];
    for i in 0..=80 {
        let distance = i as f32 * 0.5;
        assert!((fog.visibility(vec2(0.0, distance)) - range_fade(distance, 20.0)).abs() < 1e-5);
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
    let second = sight.viewers[1].visibility(point);
    assert!(second > 0.0, "{second}");
    assert_eq!(sight.visibility(point), second);
    assert_eq!(sight.visibility(vec2(2.0, 0.0)), 1.0);
}

#[test]
fn fog_eases_open_and_closed_over_the_fade_time() {
    let tank = viewer(1, Vec2::zeros());
    let open = vision(vec![tank], Vec::new());
    // The same tank, boxed in by cover all round.
    let boxed = vision(
        vec![tank],
        vec![GroundRect::axis_aligned(Vec2::zeros(), vec2(0.5, 0.5))],
    );
    let full = tank.range + RANGE_FADE_OUTSIDE;
    let mut fog = FogSight::default();
    let reach = |fog: &FogSight| fog.sight().viewers[0].reach[0];

    // The first update shows the sight as it is.
    fog.update(&boxed, 0.0);
    let closed = reach(&fog);
    assert!(closed < 0.5, "{closed}");

    // Opening up sweeps the reach out over the fade time, a third at a
    // time.
    let tick = FADE_TIME / 3.0;
    fog.update(&open, tick);
    assert!(
        (reach(&fog) - (closed + full / 3.0)).abs() < 1e-3,
        "{}",
        reach(&fog)
    );
    fog.update(&open, tick);
    fog.update(&open, tick);
    assert!((reach(&fog) - full).abs() < 1e-3);

    // Closing sweeps it back.
    fog.update(&boxed, tick);
    assert!((reach(&fog) - (full - full / 3.0)).abs() < 1e-3);
    fog.update(&boxed, tick);
    fog.update(&boxed, tick);
    assert!((reach(&fog) - closed).abs() < 1e-3);

    // A pause lets no fog time pass.
    fog.update(&open, 0.0);
    assert!((reach(&fog) - closed).abs() < 1e-3);
}

#[test]
fn a_tank_joining_the_side_opens_its_sight_and_one_leaving_closes_it() {
    let first = viewer(1, Vec2::zeros());
    let second = viewer(2, vec2(20.0, 0.0));
    let full = first.range + RANGE_FADE_OUTSIDE;
    let mut fog = FogSight::default();
    let both = vision(vec![first, second], Vec::new());
    let alone = vision(vec![first], Vec::new());
    fog.update(&alone, 0.0);
    assert_eq!(fog.sight().viewers.len(), 1);

    let tick = FADE_TIME / 3.0;
    fog.update(&both, tick);
    let joined = &fog.sight().viewers[1];
    assert_eq!(joined.position, second.position);
    assert!((joined.reach[0] - full / 3.0).abs() < 1e-3);
    assert_eq!(fog.sight().viewers[0].reach[0], full);
    fog.update(&both, tick);
    fog.update(&both, tick);
    assert!((fog.sight().viewers[1].reach[0] - full).abs() < 1e-3);

    // The second tank dies: its sight closes where it last stood, then
    // goes.
    fog.update(&alone, tick);
    fog.update(&alone, tick);
    assert_eq!(fog.sight().viewers.len(), 2);
    assert_eq!(fog.sight().viewers[1].position, second.position);
    assert!((fog.sight().viewers[1].reach[0] - full / 3.0).abs() < 1e-3);
    fog.update(&alone, tick);
    assert_eq!(fog.sight().viewers.len(), 1);
}

#[test]
fn no_seeing_tank_darkens_everything() {
    let mut fog = FogSight::default();
    fog.update(&TeamVision::default(), 0.016);
    assert!(fog.sight().viewers.is_empty());
    assert_eq!(fog.sight().visibility(Vec2::zeros()), 0.0);
}
