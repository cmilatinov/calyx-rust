use crate as engine;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::TypeUuid;
use nalgebra_glm::{vec4, Vec4};
use serde::{Deserialize, Serialize};

/// One color stop of a [`Gradient`]: its color at `time`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "0e2f0a41-7c1b-4b2e-9f51-3d6a8c0b5e27"]
#[reflect(Default)]
#[serde(default)]
pub struct GradientKey {
    /// Where the key sits on the gradient's timeline, from `0.0` to `1.0`.
    pub time: f32,
    /// Unmultiplied sRGB red, green, blue and alpha, each from `0.0` to
    /// `1.0`.
    pub color: Vec4,
}

impl Default for GradientKey {
    fn default() -> Self {
        Self::new(0.0, Gradient::EMPTY_COLOR)
    }
}

impl GradientKey {
    /// A key of `color`, unmultiplied sRGB RGBA, at `time`.
    pub fn new(time: f32, color: Vec4) -> Self {
        Self { time, color }
    }

    /// A key of the unmultiplied sRGB RGBA `color` at `time`.
    pub fn rgba(time: f32, [r, g, b, a]: [f32; 4]) -> Self {
        Self::new(time, vec4(r, g, b, a))
    }
}

/// A color over time, authored as color keys and blended linearly between
/// them, channel by channel in unmultiplied sRGB.
///
/// Gradients are meant for colors that vary over a normalized timeline, such
/// as a particle's lifetime, where time runs from `0.0` to `1.0`. Evaluation
/// holds the first key's color before it and the last key's color after it,
/// and an empty gradient is opaque white.
///
/// `keys` stay sorted by time: [`Gradient::new`], deserialization and the
/// editing methods sort them, and code that writes `keys` directly calls
/// [`Gradient::sort`] afterwards. A serialized gradient with no `keys` loads
/// as [`Gradient::default`]; one with empty `keys` loads as an empty
/// gradient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "7c0d6f1e-2a59-4f83-b8e4-91c5a2d3f640"]
#[reflect(Default)]
#[serde(from = "GradientData")]
pub struct Gradient {
    /// The gradient's keys, sorted by time.
    pub keys: Vec<GradientKey>,
}

impl Default for Gradient {
    /// Opaque white at the start, fading to transparent white at the end.
    fn default() -> Self {
        Self::new(vec![
            GradientKey::rgba(0.0, [1.0, 1.0, 1.0, 1.0]),
            GradientKey::rgba(1.0, [1.0, 1.0, 1.0, 0.0]),
        ])
    }
}

/// A [`Gradient`] as it is serialized, before its keys are sorted.
#[derive(Deserialize)]
#[serde(default)]
struct GradientData {
    keys: Vec<GradientKey>,
}

impl Default for GradientData {
    /// The keys of [`Gradient::default`], for a gradient serialized without
    /// `keys`.
    fn default() -> Self {
        Self {
            keys: Gradient::default().keys,
        }
    }
}

impl From<GradientData> for Gradient {
    fn from(data: GradientData) -> Self {
        Self::new(data.keys)
    }
}

impl Gradient {
    /// The color of an empty gradient: opaque white.
    pub const EMPTY_COLOR: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);

    /// A gradient through `keys`, sorted by time.
    pub fn new(keys: Vec<GradientKey>) -> Self {
        let mut gradient = Self { keys };
        gradient.sort();
        gradient
    }

    /// A gradient from `start` at time `0.0` to `end` at time `1.0`, each
    /// unmultiplied sRGB RGBA.
    pub fn linear(start: [f32; 4], end: [f32; 4]) -> Self {
        Self::new(vec![
            GradientKey::rgba(0.0, start),
            GradientKey::rgba(1.0, end),
        ])
    }

    /// Sorts the keys by time, keeping the order of keys at the same time.
    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }

    /// Inserts `key` in time order and returns its index.
    pub fn insert(&mut self, key: GradientKey) -> usize {
        let index = self.keys.partition_point(|other| other.time <= key.time);
        self.keys.insert(index, key);
        index
    }

    /// The color at `time`, unmultiplied sRGB RGBA.
    ///
    /// `assets/shaders/gradient.wgsl` evaluates gradients the same way on the
    /// GPU.
    pub fn evaluate(&self, time: f32) -> Vec4 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return Self::EMPTY_COLOR;
        };
        if time.is_nan() || time < first.time {
            return first.color;
        }
        if time >= last.time {
            return last.color;
        }
        let next = self.keys.partition_point(|key| key.time <= time);
        let from = &self.keys[next - 1];
        let to = &self.keys[next];
        let span = to.time - from.time;
        if span <= 0.0 {
            return to.color;
        }
        let t = (time - from.time) / span;
        from.color + (to.color - from.color) * t
    }
}

#[cfg(test)]
mod tests {
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
}
