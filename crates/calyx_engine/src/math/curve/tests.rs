use super::*;
use approx::assert_abs_diff_eq;

fn keys() -> Curve {
    Curve::new(vec![
        CurveKey::new(0.0, 0.0),
        CurveKey::new(0.5, 2.0).with_interpolation(CurveInterpolation::Constant),
        CurveKey::new(0.75, 4.0).with_interpolation(CurveInterpolation::Smooth),
        CurveKey::new(1.0, 0.0),
    ])
}

#[test]
fn empty_curve_evaluates_to_zero() {
    assert_eq!(Curve::new(Vec::new()).evaluate(0.5), 0.0);
}

#[test]
fn single_key_curve_is_constant() {
    let curve = Curve::new(vec![CurveKey::new(0.3, 7.0)]);
    assert_eq!(curve.evaluate(0.0), 7.0);
    assert_eq!(curve.evaluate(0.3), 7.0);
    assert_eq!(curve.evaluate(1.0), 7.0);
}

#[test]
fn evaluation_holds_the_end_keys_outside_the_curve() {
    let curve = Curve::new(vec![CurveKey::new(0.2, 1.0), CurveKey::new(0.8, 3.0)]);
    assert_eq!(curve.evaluate(-1.0), 1.0);
    assert_eq!(curve.evaluate(0.0), 1.0);
    assert_eq!(curve.evaluate(1.0), 3.0);
    assert_eq!(curve.evaluate(5.0), 3.0);
    assert_eq!(curve.evaluate(f32::NAN), 1.0);
}

#[test]
fn linear_keys_interpolate_in_a_straight_line() {
    let curve = keys();
    assert_abs_diff_eq!(curve.evaluate(0.25), 1.0);
    assert_abs_diff_eq!(curve.evaluate(0.5), 2.0);
    assert_abs_diff_eq!(Curve::linear(1.0, 0.0).evaluate(0.25), 0.75);
}

#[test]
fn constant_keys_hold_their_value_until_the_next_key() {
    let curve = keys();
    assert_abs_diff_eq!(curve.evaluate(0.6), 2.0);
    assert_abs_diff_eq!(curve.evaluate(0.7499), 2.0);
    assert_abs_diff_eq!(curve.evaluate(0.75), 4.0);
}

#[test]
fn smooth_keys_ease_between_values() {
    let curve = keys();
    // Smoothstep: halfway in time is halfway in value, a quarter in is
    // 0.15625 of the way.
    assert_abs_diff_eq!(curve.evaluate(0.875), 2.0);
    assert_abs_diff_eq!(curve.evaluate(0.8125), 4.0 - 4.0 * 0.15625);
}

#[test]
fn keys_at_the_same_time_jump_to_the_later_value() {
    let curve = Curve::new(vec![
        CurveKey::new(0.0, 0.0),
        CurveKey::new(0.5, 1.0),
        CurveKey::new(0.5, 3.0),
        CurveKey::new(1.0, 3.0),
    ]);
    assert_abs_diff_eq!(curve.evaluate(0.25), 0.5);
    assert_abs_diff_eq!(curve.evaluate(0.5), 3.0);
    assert_abs_diff_eq!(curve.evaluate(0.75), 3.0);
}

#[test]
fn coincident_first_keys_take_the_later_value_at_their_time() {
    let curve = Curve::new(vec![
        CurveKey::new(0.0, 1.0),
        CurveKey::new(0.0, 3.0),
        CurveKey::new(1.0, 3.0),
    ]);
    assert_abs_diff_eq!(curve.evaluate(-0.5), 1.0);
    assert_abs_diff_eq!(curve.evaluate(0.0), 3.0);
    assert_abs_diff_eq!(curve.evaluate(0.5), 3.0);

    let all = Curve::new(vec![CurveKey::new(0.5, 1.0), CurveKey::new(0.5, 2.0)]);
    assert_abs_diff_eq!(all.evaluate(0.5), 2.0);
    assert_abs_diff_eq!(all.evaluate(0.0), 1.0);
}

#[test]
fn a_segment_maps_its_keys_values_before_interpolating() {
    let curve = Curve::linear(1.0, -1.0);
    let segment = curve.segment(0.75);
    assert_eq!(
        (segment.start, segment.end, segment.weight),
        (1.0, -1.0, 0.75)
    );
    // Clamping each key, then interpolating, is not clamping the value.
    assert_abs_diff_eq!(segment.map(|value| value.max(0.0)).value(), 0.25);
    assert_eq!(curve.segment(2.0), curve.segment(1.0));
    assert_eq!(curve.segment(2.0).weight, 0.0);
    assert_eq!(Curve::new(Vec::new()).segment(0.5).value(), 0.0);
}

#[test]
fn new_and_insert_keep_keys_sorted() {
    let mut curve = Curve::new(vec![CurveKey::new(1.0, 1.0), CurveKey::new(0.0, 0.0)]);
    assert_eq!(curve.keys[0].time, 0.0);
    assert_eq!(curve.insert(CurveKey::new(0.5, 4.0)), 1);
    let times: Vec<f32> = curve.keys.iter().map(|key| key.time).collect();
    assert_eq!(times, vec![0.0, 0.5, 1.0]);
    assert_eq!(curve.value_range(), Some((0.0, 4.0)));
}

#[test]
fn curve_round_trips_through_json() {
    let curve = keys();
    let json = serde_json::to_string(&curve).unwrap();
    let back: Curve = serde_json::from_str(&json).unwrap();
    assert_eq!(back, curve);
}

#[test]
fn default_curve_round_trips_through_json() {
    let curve = Curve::default();
    let json = serde_json::to_string(&curve).unwrap();
    let back: Curve = serde_json::from_str(&json).unwrap();
    assert_eq!(back, curve);
}

#[test]
fn curve_without_keys_deserializes_to_the_default_curve() {
    let curve: Curve = serde_json::from_str("{}").unwrap();
    assert_eq!(curve, Curve::default());
}

#[test]
fn curve_with_explicit_empty_keys_deserializes_empty() {
    let curve: Curve = serde_json::from_str(r#"{"keys":[]}"#).unwrap();
    assert!(curve.keys.is_empty());
}

#[test]
fn deserialization_sorts_keys_and_fills_defaults() {
    let curve: Curve = serde_json::from_str(
        r#"{"keys":[{"time":1.0,"value":2.0},{"time":0.0,"value":1.0,"interpolation":"Smooth"}]}"#,
    )
    .unwrap();
    assert_eq!(
        curve.keys,
        vec![
            CurveKey::new(0.0, 1.0).with_interpolation(CurveInterpolation::Smooth),
            CurveKey::new(1.0, 2.0),
        ]
    );
}
