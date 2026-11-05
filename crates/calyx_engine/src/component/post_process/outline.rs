//! The ink outline effect component, read by the outline pass.

use super::{PostProcessEffect, ReflectPostProcessEffect};
use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use serde::{Deserialize, Serialize};

/// Ink outlines: lines of a constant width in pixels, drawn after tone
/// mapping around and inside the opaque PBR surfaces whose material picks
/// one of [`Self::styles`].
///
/// A material's `outline_style` is 0 for no outline, or `N` for
/// `styles[N - 1]`. An edge takes the nearer surface's style; a nearer surface
/// of style 0 hides the farther surface's ink. Silhouettes (a surface against the
/// background, a large jump in distance, or a change of style) draw at the
/// style's `width` in full ink; inner lines (a crease, or a small step in
/// distance) draw at its `inner_width`, at `inner_opacity`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "bac082cd-d184-4584-8b58-b1a652577c79"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
#[reflect_attr(name = "Outline", category = "Post Processing")]
#[serde(default)]
pub struct ComponentOutline {
    /// The styles materials pick by index: a material's `outline_style` `N`
    /// is entry `N - 1`. At most [`MAX_OUTLINE_STYLES`] are drawn. The
    /// inspector labels the entries "Style 1", "Style 2", ..., the numbers
    /// materials use.
    #[reflect_attr(element_label = "Style", element_base = 1)]
    pub styles: Vec<OutlineStyle>,
    /// The smallest jump in distance across an edge, as a fraction of the
    /// distance, that draws an inner line. Measured against the distance the
    /// surfaces on either side predict, so a plane seen at a grazing angle
    /// draws none.
    #[reflect_attr(min = 0.0, speed = 0.0005)]
    pub depth_threshold: f32,
    /// The smallest jump in distance, measured the same way, that makes the
    /// edge a silhouette: two different surfaces, one in front of the other.
    #[reflect_attr(min = 0.0, speed = 0.0005)]
    pub silhouette_threshold: f32,
    /// The smallest angle, in degrees, between the normals on either side of
    /// an edge that draws an inner line. Smooth normals that turn more gently
    /// than this from one pixel to the next, as on rounded shapes, draw none.
    #[reflect_attr(min = 0.0, max = 180.0, speed = 0.5)]
    pub normal_threshold: f32,
}

/// The most outline styles the pass draws; entries past it are ignored.
pub const MAX_OUTLINE_STYLES: usize = 8;

impl PostProcessEffect for ComponentOutline {}

impl Component for ComponentOutline {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentOutline {
    /// No styles: until a style is added, no material is outlined.
    fn default() -> Self {
        Self {
            styles: Vec::new(),
            depth_threshold: 0.01,
            silhouette_threshold: 0.04,
            normal_threshold: 40.0,
        }
    }
}

/// One look of ink outline, which materials pick by index.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "4d1f0b52-8f7e-4a43-9a64-3c0c4e0f9a21"]
#[reflect(Default)]
#[serde(default)]
pub struct OutlineStyle {
    /// The ink colour, sRGB, shown as it is: the lines draw after tone
    /// mapping and colour grading.
    pub color: Color32,
    /// Silhouette width in pixels, rounded to a whole number from 1 to 8.
    /// A line straddles its edge; an odd width puts the extra pixel on the
    /// nearer surface.
    #[reflect_attr(min = 1.0, max = 8.0, speed = 0.05)]
    pub width: f32,
    /// Inner line width in pixels, rounded the same way.
    #[reflect_attr(min = 1.0, max = 8.0, speed = 0.05)]
    pub inner_width: f32,
    /// How much of the ink an inner line takes, relative to a silhouette.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub inner_opacity: f32,
    /// How much of the ink colour a line takes, from 0 (no lines) to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub opacity: f32,
}

impl Default for OutlineStyle {
    /// The tank game's ink: `#121212`, 2 px silhouettes, 1 px inner lines at
    /// 0.55.
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(0x12, 0x12, 0x12),
            width: 2.0,
            inner_width: 1.0,
            inner_opacity: 0.55,
            opacity: 1.0,
        }
    }
}
