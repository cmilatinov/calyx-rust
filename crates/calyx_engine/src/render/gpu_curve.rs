//! [`Curve`]s as `assets/shaders/common/curve.wgsl` reads them: keys packed one curve
//! after another in one buffer, and a range per curve.

use crate::math::{Curve, CurveInterpolation, CurveKey};

/// `CurveKey` of `curve.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCurveKey {
    pub time: f32,
    pub value: f32,
    /// One of [`GpuCurveKey::CONSTANT`], [`GpuCurveKey::LINEAR`] or
    /// [`GpuCurveKey::SMOOTH`].
    pub interpolation: u32,
    _padding: u32,
}

impl GpuCurveKey {
    /// `CURVE_CONSTANT` of `curve.wgsl`.
    pub const CONSTANT: u32 = 0;
    /// `CURVE_LINEAR` of `curve.wgsl`.
    pub const LINEAR: u32 = 1;
    /// `CURVE_SMOOTH` of `curve.wgsl`.
    pub const SMOOTH: u32 = 2;
}

impl From<&CurveKey> for GpuCurveKey {
    fn from(key: &CurveKey) -> Self {
        Self {
            time: key.time,
            value: key.value,
            interpolation: match key.interpolation {
                CurveInterpolation::Constant => Self::CONSTANT,
                CurveInterpolation::Linear => Self::LINEAR,
                CurveInterpolation::Smooth => Self::SMOOTH,
            },
            _padding: 0,
        }
    }
}

/// `CurveRange` of `curve.wgsl`: the keys of one curve in a key buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCurveRange {
    /// Index of the curve's first key.
    pub offset: u32,
    /// Number of keys.
    pub count: u32,
    _padding: [u32; 2],
}

/// The contents of a curve key buffer: curves packed one after another, each
/// with its keys in time order, as [`Curve`] keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuCurveKeys {
    keys: Vec<GpuCurveKey>,
}

impl GpuCurveKeys {
    /// An empty key buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Packs `curves` in order and returns the keys with each curve's range.
    pub fn pack<'a>(curves: impl IntoIterator<Item = &'a Curve>) -> (Self, Vec<GpuCurveRange>) {
        let mut keys = Self::new();
        let ranges = curves.into_iter().map(|curve| keys.push(curve)).collect();
        (keys, ranges)
    }

    /// Appends `curve`'s keys and returns their range.
    pub fn push(&mut self, curve: &Curve) -> GpuCurveRange {
        let range = GpuCurveRange {
            offset: self.keys.len() as u32,
            count: curve.keys.len() as u32,
            _padding: [0; 2],
        };
        self.keys.extend(curve.keys.iter().map(GpuCurveKey::from));
        range
    }

    /// The packed keys.
    pub fn keys(&self) -> &[GpuCurveKey] {
        &self.keys
    }

    /// The buffer contents: the packed keys, or one zeroed key when there are
    /// none, since a bound buffer cannot be empty.
    pub fn contents(&self) -> &[u8] {
        if self.keys.is_empty() {
            bytemuck::bytes_of(&EMPTY_KEY)
        } else {
            bytemuck::cast_slice(&self.keys)
        }
    }
}

static EMPTY_KEY: GpuCurveKey = GpuCurveKey {
    time: 0.0,
    value: 0.0,
    interpolation: 0,
    _padding: 0,
};

#[cfg(test)]
mod tests;
