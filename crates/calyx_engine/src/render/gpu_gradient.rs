//! [`Gradient`]s as `assets/shaders/common/gradient.wgsl` reads them: keys packed
//! one gradient after another in one buffer, and a range per gradient.

use crate::math::{Gradient, GradientKey};

/// `GradientKey` of `gradient.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuGradientKey {
    /// Unmultiplied sRGB RGBA.
    pub color: [f32; 4],
    pub time: f32,
    _padding: [u32; 3],
}

impl From<&GradientKey> for GpuGradientKey {
    fn from(key: &GradientKey) -> Self {
        Self {
            color: key.color.into(),
            time: key.time,
            _padding: [0; 3],
        }
    }
}

/// `GradientRange` of `gradient.wgsl`: the keys of one gradient in a key
/// buffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuGradientRange {
    /// Index of the gradient's first key.
    pub offset: u32,
    /// Number of keys.
    pub count: u32,
    _padding: [u32; 2],
}

/// The contents of a gradient key buffer: gradients packed one after
/// another, each with its keys in time order, as [`Gradient`] keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuGradientKeys {
    keys: Vec<GpuGradientKey>,
}

impl GpuGradientKeys {
    /// An empty key buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Packs `gradients` in order and returns the keys with each gradient's
    /// range.
    pub fn pack<'a>(
        gradients: impl IntoIterator<Item = &'a Gradient>,
    ) -> (Self, Vec<GpuGradientRange>) {
        let mut keys = Self::new();
        let ranges = gradients
            .into_iter()
            .map(|gradient| keys.push(gradient))
            .collect();
        (keys, ranges)
    }

    /// Appends `gradient`'s keys and returns their range.
    pub fn push(&mut self, gradient: &Gradient) -> GpuGradientRange {
        let range = GpuGradientRange {
            offset: self.keys.len() as u32,
            count: gradient.keys.len() as u32,
            _padding: [0; 2],
        };
        self.keys
            .extend(gradient.keys.iter().map(GpuGradientKey::from));
        range
    }

    /// The packed keys.
    pub fn keys(&self) -> &[GpuGradientKey] {
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

static EMPTY_KEY: GpuGradientKey = GpuGradientKey {
    color: [0.0; 4],
    time: 0.0,
    _padding: [0; 3],
};

#[cfg(test)]
mod tests;
