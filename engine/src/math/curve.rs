use crate as engine;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::TypeUuid;
use serde::{Deserialize, Deserializer, Serialize};

/// How a [`Curve`] moves from one key to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a3dc0e41-613e-46c6-8002-895f5f2ececb"]
#[reflect(Default)]
#[repr(C)]
pub enum CurveInterpolation {
    /// Holds the key's value until the next key.
    Constant,
    /// Moves in a straight line to the next key's value.
    #[default]
    Linear,
    /// Eases out of the key and into the next one (smoothstep).
    Smooth,
}

/// One point of a [`Curve`]: its value at `time`, and how the curve moves
/// from it to the next key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "35d039a9-828b-4f2b-abc8-da8c5af2b028"]
#[reflect(Default)]
#[serde(default)]
#[repr(C)]
pub struct CurveKey {
    /// Where the key sits on the curve's timeline.
    pub time: f32,
    /// The curve's value at `time`.
    pub value: f32,
    /// How the curve moves from this key to the next one.
    pub interpolation: CurveInterpolation,
}

impl CurveKey {
    /// A key with linear interpolation.
    pub fn new(time: f32, value: f32) -> Self {
        Self {
            time,
            value,
            interpolation: CurveInterpolation::Linear,
        }
    }

    /// This key with `interpolation`.
    pub fn with_interpolation(mut self, interpolation: CurveInterpolation) -> Self {
        self.interpolation = interpolation;
        self
    }
}

/// A scalar value over time, authored as keys and evaluated between them.
///
/// Curves are meant for values that vary over a normalized timeline, such as
/// a particle's lifetime, where time runs from `0.0` to `1.0`; the editor's
/// curve widget edits keys in that range. Evaluation holds the first key's
/// value before it and the last key's value after it, and an empty curve
/// evaluates to `0.0`.
///
/// `keys` stay sorted by time: [`Curve::new`], deserialization and the
/// editing methods sort them, and code that writes `keys` directly calls
/// [`Curve::sort`] afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, TypeUuid, Reflect)]
#[uuid = "5ec3b58f-4ea5-4ecb-9a22-ba19772a6331"]
#[reflect(Default)]
#[repr(C)]
pub struct Curve {
    /// The curve's keys, sorted by time.
    pub keys: Vec<CurveKey>,
}

impl Default for Curve {
    /// A straight line from `0.0` at the start to `1.0` at the end.
    fn default() -> Self {
        Self::linear(0.0, 1.0)
    }
}

impl<'de> Deserialize<'de> for Curve {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct CurveData {
            #[serde(default)]
            keys: Vec<CurveKey>,
        }
        Ok(Self::new(CurveData::deserialize(deserializer)?.keys))
    }
}

impl Curve {
    /// A curve through `keys`, sorted by time.
    pub fn new(keys: Vec<CurveKey>) -> Self {
        let mut curve = Self { keys };
        curve.sort();
        curve
    }

    /// A curve that is `value` everywhere.
    pub fn constant(value: f32) -> Self {
        Self::new(vec![CurveKey::new(0.0, value), CurveKey::new(1.0, value)])
    }

    /// A straight line from `start` at time `0.0` to `end` at time `1.0`.
    pub fn linear(start: f32, end: f32) -> Self {
        Self::new(vec![CurveKey::new(0.0, start), CurveKey::new(1.0, end)])
    }

    /// Sorts the keys by time, keeping the order of keys at the same time.
    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }

    /// Inserts `key` in time order and returns its index.
    pub fn insert(&mut self, key: CurveKey) -> usize {
        let index = self.keys.partition_point(|other| other.time <= key.time);
        self.keys.insert(index, key);
        index
    }

    /// The curve's value at `time`.
    pub fn evaluate(&self, time: f32) -> f32 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else {
            return 0.0;
        };
        if time.is_nan() || time <= first.time {
            return first.value;
        }
        if time >= last.time {
            return last.value;
        }
        let next = self.keys.partition_point(|key| key.time <= time);
        let from = &self.keys[next - 1];
        let to = &self.keys[next];
        let span = to.time - from.time;
        if span <= 0.0 {
            return to.value;
        }
        let t = (time - from.time) / span;
        let t = match from.interpolation {
            CurveInterpolation::Constant => 0.0,
            CurveInterpolation::Linear => t,
            CurveInterpolation::Smooth => t * t * (3.0 - 2.0 * t),
        };
        from.value + (to.value - from.value) * t
    }

    /// The smallest and largest key values, or `None` for an empty curve.
    pub fn value_range(&self) -> Option<(f32, f32)> {
        self.keys.iter().fold(None, |range, key| match range {
            None => Some((key.value, key.value)),
            Some((min, max)) => Some((min.min(key.value), max.max(key.value))),
        })
    }
}

#[cfg(test)]
mod tests {
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
        let empty: Curve = serde_json::from_str("{}").unwrap();
        assert!(empty.keys.is_empty());
    }
}
