use crate as calyx_engine;
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
    /// `assets/shaders/common/gradient.wgsl` evaluates gradients the same way on the
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
mod tests;
