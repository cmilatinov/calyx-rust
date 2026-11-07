//! Per-field blending of the post-process effect components, used to blend a
//! local volume's effects over the settings below it.

use crate::component::{
    ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom, ComponentColorGrading,
    ComponentDepthOfField, ComponentFog, ComponentFogOfWar, ComponentOutline, ComponentTonemap,
    ComponentVignette, OutlineStyle, MAX_OUTLINE_STYLES,
};
use egui::{Color32, Rgba};

/// An effect component a local volume can blend over the value below it.
///
/// `f32` fields and colours interpolate, colours in linear space; integer
/// and enum fields switch to the target at a factor of 0.5.
pub trait BlendEffect: Clone {
    /// `self` blended toward `target` by `factor`: `self` at 0, `target` at
    /// 1.
    fn blend(&self, target: &Self, factor: f32) -> Self;

    /// The values the effect fades in from toward `target` when it is off
    /// below a volume that turns it on: `target` with the effect's strength
    /// at its neutral value. `None` for an effect with no neutral value,
    /// which switches on at a factor of 0.5 instead.
    fn neutral(target: &Self) -> Option<Self>;

    /// Copies into this effect's existing storage.
    fn copy_in_place(&mut self, source: &Self) {
        self.clone_from(source);
    }

    /// Blends into this effect's existing storage with the rules of `blend`.
    fn blend_in_place(&mut self, target: &Self, factor: f32) {
        *self = self.blend(target, factor);
    }

    /// Resets to `neutral`, or leaves the effect alone when it has no neutral.
    fn reset_neutral_in_place(&mut self) -> bool {
        if let Some(neutral) = Self::neutral(self) {
            *self = neutral;
            true
        } else {
            false
        }
    }
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

impl BlendEffect for ComponentFog {
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            mode: switch(self.mode, target.mode, factor),
            color: lerp_color(self.color, target.color, factor),
            intensity: lerp(self.intensity, target.intensity, factor),
            start: lerp(self.start, target.start, factor),
            end: lerp(self.end, target.end, factor),
            density: lerp(self.density, target.density, factor),
            opacity: lerp(self.opacity, target.opacity, factor),
        }
    }

    /// No fog: nothing takes any of the fog colour.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            opacity: 0.0,
            ..*target
        })
    }
}

impl BlendEffect for ComponentFogOfWar {
    fn copy_in_place(&mut self, source: &Self) {
        self.color = source.color;
        self.opacity = source.opacity;
        // Reuses the viewers' reach storage once warm.
        self.sight.clone_from(&source.sight);
    }

    /// The colour and opacity interpolate. The sight is game state, not a
    /// look, so it switches to the target's at 0.5, and stays where the
    /// target has none.
    fn blend(&self, target: &Self, factor: f32) -> Self {
        let sight = match (&self.sight, &target.sight) {
            (Some(_), None) => self.sight.clone(),
            _ => switch(&self.sight, &target.sight, factor).clone(),
        };
        Self {
            color: lerp_color(self.color, target.color, factor),
            opacity: lerp(self.opacity, target.opacity, factor),
            sight,
        }
    }

    /// No darkening.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            opacity: 0.0,
            ..target.clone()
        })
    }

    fn reset_neutral_in_place(&mut self) -> bool {
        self.opacity = 0.0;
        true
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

impl BlendEffect for ComponentOutline {
    fn copy_in_place(&mut self, source: &Self) {
        // Reserve the full supported style table on the first copy, even when
        // that volume supplies only one style. Later supported-size edits
        // must not allocate in the per-view settings resolver.
        self.styles
            .reserve(MAX_OUTLINE_STYLES.saturating_sub(self.styles.len()));
        self.styles.clone_from(&source.styles);
        self.depth_threshold = source.depth_threshold;
        self.silhouette_threshold = source.silhouette_threshold;
        self.normal_threshold = source.normal_threshold;
    }

    fn blend_in_place(&mut self, target: &Self, factor: f32) {
        let count = self.styles.len().max(target.styles.len());
        for index in 0..count {
            match (self.styles.get_mut(index), target.styles.get(index)) {
                (Some(from), Some(to)) => *from = from.blend(to, factor),
                (Some(from), None) => *from = from.blend(&from.invisible(), factor),
                (None, Some(to)) => self.styles.push(to.invisible().blend(to, factor)),
                (None, None) => unreachable!("index is below the longer list's length"),
            }
        }
        self.depth_threshold = lerp(self.depth_threshold, target.depth_threshold, factor);
        self.silhouette_threshold = lerp(
            self.silhouette_threshold,
            target.silhouette_threshold,
            factor,
        );
        self.normal_threshold = lerp(self.normal_threshold, target.normal_threshold, factor);
    }

    fn reset_neutral_in_place(&mut self) -> bool {
        for style in &mut self.styles {
            style.opacity = 0.0;
        }
        true
    }

    /// The thresholds interpolate, and the styles blend entry by entry, by
    /// index, over the longer of the two lists: an entry one side lacks
    /// blends from, or to, an invisible copy of the other side's entry (its
    /// values at opacity 0), so a style a volume adds fades in and one it
    /// lacks fades out.
    fn blend(&self, target: &Self, factor: f32) -> Self {
        let count = self.styles.len().max(target.styles.len());
        let styles = (0..count)
            .map(
                |index| match (self.styles.get(index), target.styles.get(index)) {
                    (Some(from), Some(to)) => from.blend(to, factor),
                    (Some(from), None) => from.blend(&from.invisible(), factor),
                    (None, Some(to)) => to.invisible().blend(to, factor),
                    (None, None) => unreachable!("index is below the longer list's length"),
                },
            )
            .collect();
        Self {
            styles,
            depth_threshold: lerp(self.depth_threshold, target.depth_threshold, factor),
            silhouette_threshold: lerp(
                self.silhouette_threshold,
                target.silhouette_threshold,
                factor,
            ),
            normal_threshold: lerp(self.normal_threshold, target.normal_threshold, factor),
        }
    }

    /// No lines: every style at opacity 0.
    fn neutral(target: &Self) -> Option<Self> {
        Some(Self {
            styles: target.styles.iter().map(OutlineStyle::invisible).collect(),
            ..target.clone()
        })
    }
}

impl OutlineStyle {
    /// `self` blended toward `target` by `factor`: colours in linear space,
    /// widths as floats (the pass rounds them), opacities linearly.
    fn blend(&self, target: &Self, factor: f32) -> Self {
        Self {
            color: lerp_color(self.color, target.color, factor),
            width: lerp(self.width, target.width, factor),
            inner_width: lerp(self.inner_width, target.inner_width, factor),
            inner_opacity: lerp(self.inner_opacity, target.inner_opacity, factor),
            opacity: lerp(self.opacity, target.opacity, factor),
        }
    }

    /// `self` drawing nothing.
    fn invisible(&self) -> Self {
        Self {
            opacity: 0.0,
            ..*self
        }
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
