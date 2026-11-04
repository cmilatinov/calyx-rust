//! The ink outline effect component, read by the outline pass.

use super::{PostProcessEffect, ReflectPostProcessEffect};
use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use serde::{Deserialize, Serialize};

/// Ink outlines: lines of a constant width in pixels, drawn after tone
/// mapping where the distance to the opaque surfaces jumps (silhouettes) or
/// their normals turn sharply (creases).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "bac082cd-d184-4584-8b58-b1a652577c79"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
#[reflect_attr(name = "Outline", category = "Post Processing")]
#[serde(default)]
pub struct ComponentOutline {
    /// The ink colour, sRGB, shown as it is: the lines draw after tone
    /// mapping and colour grading.
    pub color: Color32,
    /// Line width in pixels, rounded to a whole number from 1 to 8. A line
    /// straddles the edge; an odd width puts the extra pixel on the nearer
    /// surface.
    #[reflect_attr(min = 1.0, max = 8.0, speed = 0.05)]
    pub width: f32,
    /// The smallest jump in distance across an edge, as a fraction of the
    /// distance, that draws a line. Measured against the distance the
    /// surfaces on either side predict, so a plane seen at a grazing angle
    /// draws none.
    #[reflect_attr(min = 0.0, speed = 0.0005)]
    pub depth_threshold: f32,
    /// The smallest angle, in degrees, between the normals on either side of
    /// an edge that draws a line. Smooth normals that turn more gently than
    /// this from one pixel to the next, as on rounded shapes, draw none.
    #[reflect_attr(min = 0.0, max = 180.0, speed = 0.5)]
    pub normal_threshold: f32,
    /// How much of the ink colour a line takes, from 0 (no lines) to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub opacity: f32,
}

impl PostProcessEffect for ComponentOutline {}

impl Component for ComponentOutline {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentOutline {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(0x12, 0x12, 0x12),
            width: 2.0,
            depth_threshold: 0.01,
            normal_threshold: 40.0,
            opacity: 1.0,
        }
    }
}
