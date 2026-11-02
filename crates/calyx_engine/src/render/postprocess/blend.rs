//! Per-field blending of the post-process effect components, used to blend a
//! local volume's effects over the settings below it.

use crate::component::{
    ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom, ComponentColorGrading,
    ComponentDepthOfField, ComponentTonemap, ComponentVignette,
};
use egui::{Color32, Rgba};

/// An effect component a local volume can blend over the value below it.
///
/// `f32` fields and colours interpolate, colours in linear space; integer
/// and enum fields switch to the target at a factor of 0.5.
pub trait BlendEffect: Copy {
    /// `self` blended toward `target` by `factor`: `self` at 0, `target` at
    /// 1.
    fn blend(&self, target: &Self, factor: f32) -> Self;

    /// The values the effect fades in from toward `target` when it is off
    /// below a volume that turns it on: `target` with the effect's strength
    /// at its neutral value. `None` for an effect with no neutral value,
    /// which switches on at a factor of 0.5 instead.
    fn neutral(target: &Self) -> Option<Self>;
}

/// `below` with a volume's `effect` blended over it by `factor`.
///
/// An effect the volume does not have inherits `below`. One that is off
/// below fades in from its [neutral](BlendEffect::neutral) values, or
/// switches on at 0.5 when it has none.
pub fn blend_effect<T: BlendEffect>(below: Option<T>, effect: Option<T>, factor: f32) -> Option<T> {
    let Some(effect) = effect else {
        return below;
    };
    match below.or_else(|| T::neutral(&effect)) {
        Some(below) => Some(below.blend(&effect, factor)),
        None => switch(None, Some(effect), factor),
    }
}

/// `a` at 0 and exactly `b` at 1.
fn lerp(a: f32, b: f32, factor: f32) -> f32 {
    a * (1.0 - factor) + b * factor
}

/// `a` below a factor of 0.5, `b` from it.
pub(crate) fn switch<T>(a: T, b: T, factor: f32) -> T {
    if factor >= 0.5 {
        b
    } else {
        a
    }
}

/// `a` blended toward `b` in linear space.
fn lerp_color(a: Color32, b: Color32, factor: f32) -> Color32 {
    if factor <= 0.0 {
        return a;
    }
    if factor >= 1.0 {
        return b;
    }
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    Color32::from(Rgba::from_rgba_premultiplied(
        lerp(a.r(), b.r(), factor),
        lerp(a.g(), b.g(), factor),
        lerp(a.b(), b.b(), factor),
        lerp(a.a(), b.a(), factor),
    ))
}

impl BlendEffect for ComponentTonemap {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            preset: switch(self.preset, target.preset, factor),
            exposure_ev: lerp(self.exposure_ev, target.exposure_ev, factor),
        }
    }

    /// Tone mapping always runs below a volume, so it never fades in.
    fn neutral(target: &Self) -> Option<Self> {
        Some(*target)
    }
}

impl BlendEffect for ComponentAmbientOcclusion {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            radius: lerp(self.radius, target.radius, factor),
            intensity: lerp(self.intensity, target.intensity, factor),
            bias: lerp(self.bias, target.bias, factor),
            sample_count: switch(self.sample_count, target.sample_count, factor),
        }
    }

    /// No darkening.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            intensity: 0.0,
            ..*target
        })
    }
}

impl BlendEffect for ComponentDepthOfField {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            focus_distance: lerp(self.focus_distance, target.focus_distance, factor),
            focus_range: lerp(self.focus_range, target.focus_range, factor),
            max_blur: lerp(self.max_blur, target.max_blur, factor),
        }
    }

    /// No blur.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            max_blur: 0.0,
            ..*target
        })
    }
}

impl BlendEffect for ComponentBloom {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            threshold: lerp(self.threshold, target.threshold, factor),
            knee: lerp(self.knee, target.knee, factor),
            intensity: lerp(self.intensity, target.intensity, factor),
            radius: switch(self.radius, target.radius, factor),
        }
    }

    /// No light added back.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            intensity: 0.0,
            ..*target
        })
    }
}

impl BlendEffect for ComponentColorGrading {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            contrast: lerp(self.contrast, target.contrast, factor),
            saturation: lerp(self.saturation, target.saturation, factor),
            temperature: lerp(self.temperature, target.temperature, factor),
            tint: lerp(self.tint, target.tint, factor),
        }
    }

    /// The defaults, which leave the image as it is.
    fn neutral(_target: &Self) -> Option<Self> {
        Some(Self::default())
    }
}

impl BlendEffect for ComponentVignette {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            intensity: lerp(self.intensity, target.intensity, factor),
            smoothness: lerp(self.smoothness, target.smoothness, factor),
            color: lerp_color(self.color, target.color, factor),
        }
    }

    /// No darkened edges.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            intensity: 0.0,
            ..*target
        })
    }
}

impl BlendEffect for ComponentAntiAliasing {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            edge_threshold: lerp(self.edge_threshold, target.edge_threshold, factor),
            subpixel: lerp(self.subpixel, target.subpixel, factor),
        }
    }

    /// FXAA has no setting that leaves the image as it is.
    fn neutral(_target: &Self) -> Option<Self> {
        None
    }
}
