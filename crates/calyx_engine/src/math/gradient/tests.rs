use super::*;
use approx::assert_abs_diff_eq;

fn key(time: f32, color: [f32; 4]) -> GradientKey {
    GradientKey::rgba(time, color)
}

fn assert_color(actual: Vec4, expected: [f32; 4]) {
    for channel in 0..4 {
        assert_abs_diff_eq!(actual[channel], expected[channel], epsilon = 1e-6);
    }
}

#[test]
fn empty_gradient_is_opaque_white() {
    assert_eq!(
        Gradient::new(Vec::new()).evaluate(0.5),
        vec4(1.0, 1.0, 1.0, 1.0)
    );
}

#[test]
fn default_gradient_fades_white_out() {
    let gradient = Gradient::default();
    assert_color(gradient.evaluate(0.0), [1.0, 1.0, 1.0, 1.0]);
    assert_color(gradient.evaluate(0.25), [1.0, 1.0, 1.0, 0.75]);
    assert_color(gradient.evaluate(1.0), [1.0, 1.0, 1.0, 0.0]);
}

#[test]
fn evaluation_holds_the_end_keys_outside_the_gradient() {
    let gradient = Gradient::new(vec![
        key(0.2, [1.0, 0.0, 0.0, 1.0]),
        key(0.8, [0.0, 0.0, 1.0, 0.5]),
    ]);
    assert_color(gradient.evaluate(-1.0), [1.0, 0.0, 0.0, 1.0]);
    assert_color(gradient.evaluate(0.1), [1.0, 0.0, 0.0, 1.0]);
    assert_color(gradient.evaluate(f32::NAN), [1.0, 0.0, 0.0, 1.0]);
    assert_color(gradient.evaluate(0.9), [0.0, 0.0, 1.0, 0.5]);
    assert_color(gradient.evaluate(3.0), [0.0, 0.0, 1.0, 0.5]);
}

#[test]
fn keys_blend_linearly_channel_by_channel() {
    let gradient = Gradient::new(vec![
        key(0.0, [1.0, 0.0, 0.0, 1.0]),
        key(0.5, [0.0, 1.0, 0.0, 1.0]),
        key(1.0, [0.0, 0.0, 1.0, 0.0]),
    ]);
    assert_color(gradient.evaluate(0.25), [0.5, 0.5, 0.0, 1.0]);
    assert_color(gradient.evaluate(0.5), [0.0, 1.0, 0.0, 1.0]);
    assert_color(gradient.evaluate(0.75), [0.0, 0.5, 0.5, 0.5]);
}

#[test]
fn keys_at_the_same_time_jump_to_the_later_color() {
    let gradient = Gradient::new(vec![
        key(0.0, [0.0, 0.0, 0.0, 1.0]),
        key(0.5, [1.0, 1.0, 1.0, 1.0]),
        key(0.5, [1.0, 0.0, 0.0, 1.0]),
        key(1.0, [1.0, 0.0, 0.0, 1.0]),
    ]);
    assert_color(gradient.evaluate(0.25), [0.5, 0.5, 0.5, 1.0]);
    assert_color(gradient.evaluate(0.5), [1.0, 0.0, 0.0, 1.0]);
    let all = Gradient::new(vec![key(0.5, [0.0; 4]), key(0.5, [1.0; 4])]);
    assert_color(all.evaluate(0.5), [1.0; 4]);
    assert_color(all.evaluate(0.0), [0.0; 4]);
}

#[test]
fn new_and_insert_keep_keys_sorted() {
    let mut gradient = Gradient::new(vec![key(1.0, [1.0; 4]), key(0.0, [0.0; 4])]);
    assert_eq!(gradient.keys[0].time, 0.0);
    assert_eq!(gradient.insert(key(0.5, [0.5; 4])), 1);
    let times: Vec<f32> = gradient.keys.iter().map(|key| key.time).collect();
    assert_eq!(times, vec![0.0, 0.5, 1.0]);
}

#[test]
fn gradient_round_trips_through_json() {
    let gradient = Gradient::new(vec![
        key(0.0, [1.0, 0.5, 0.25, 1.0]),
        key(0.4, [0.2, 0.3, 0.4, 0.5]),
    ]);
    let json = serde_json::to_value(&gradient).unwrap();
    assert_eq!(
        json["keys"][1]["color"],
        serde_json::json!([0.2f32, 0.3f32, 0.4f32, 0.5f32])
    );
    let back: Gradient = serde_json::from_value(json).unwrap();
    assert_eq!(back, gradient);
}

#[test]
fn gradient_without_keys_deserializes_to_the_default_gradient() {
    let gradient: Gradient = serde_json::from_str("{}").unwrap();
    assert_eq!(gradient, Gradient::default());
    let empty: Gradient = serde_json::from_str(r#"{"keys":[]}"#).unwrap();
    assert!(empty.keys.is_empty());
}

#[test]
fn deserialization_sorts_keys() {
    let gradient: Gradient = serde_json::from_str(
        r#"{"keys":[{"time":1.0,"color":[0,0,0,1]},{"time":0.0,"color":[1,1,1,1]}]}"#,
    )
    .unwrap();
    assert_eq!(gradient.keys[0].time, 0.0);
    assert_eq!(gradient.keys[0].color, vec4(1.0, 1.0, 1.0, 1.0));
}
