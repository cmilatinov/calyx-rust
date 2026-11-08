//! The anti-aliasing effect component, read by the FXAA pass.

use super::{PostProcessEffect, ReflectPostProcessEffect};
use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Screen-space anti-aliasing.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    TypeUuid,
    Component,
    Reflect,
    PostProcessEffect,
)]
#[uuid = "c19ff2b7-c3ed-4f04-9dfa-e026efdedf82"]
#[reflect(Default, TypeUuidDynamic, Component, PostProcessEffect)]
#[reflect_attr(name = "Anti-Aliasing (FXAA)", category = "Post Processing")]
#[serde(default)]
pub struct ComponentAntiAliasing {
    /// Minimum local contrast, relative to the brightest neighbour, that FXAA
    /// treats as an edge. Lower smooths more edges and costs more.
    #[reflect_attr(min = 0.063, max = 0.333, speed = 0.001)]
    pub edge_threshold: f32,
    /// How much sub-pixel aliasing FXAA removes, from 0 (none) to 1
    /// (softest).
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub subpixel: f32,
    /// How far FXAA searches along an edge, and how faint an edge it treats.
    pub quality: FxaaQuality,
}

impl Component for ComponentAntiAliasing {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentAntiAliasing {
    fn default() -> Self {
        // FXAA 3.11's default quality settings.
        Self {
            edge_threshold: 0.166,
            subpixel: 0.75,
            quality: FxaaQuality::Low,
        }
    }
}

/// FXAA 3.11 Quality presets: how far the search along an edge reaches, in
/// how fine steps, and the contrast below which nothing is an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "c3746dce-29bb-435b-b2f5-e862a487fa71"]
pub enum FxaaQuality {
    /// Preset 12, the reference's default: 5 search steps reaching 20.5
    /// pixels each way, sampled up to 8.5, and no edge below a contrast of
    /// 0.0833.
    #[default]
    Low,
    /// Preset 39, the reference's highest: 12 search steps reaching 26.5
    /// pixels each way, sampled one pixel at a time for the first 5 and up
    /// to 18.5, and edges down to a contrast of 0.0312. Smooths long, thin
    /// and dark edges that `Low` leaves aliased, at a higher cost.
    High,
}

impl FxaaQuality {
    /// The longest search a preset takes, in steps.
    pub const MAX_SEARCH_STEPS: usize = 12;

    /// The preset's search step sizes along the edge, in pixels.
    pub fn search_step_sizes(self) -> &'static [f32] {
        match self {
            Self::Low => &[1.0, 1.5, 2.0, 4.0, 12.0],
            Self::High => &[1.0, 1.0, 1.0, 1.0, 1.0, 1.5, 2.0, 2.0, 2.0, 2.0, 4.0, 8.0],
        }
    }

    /// Local contrast below which nothing is an edge, whatever the edge
    /// threshold, so dark noise is left alone.
    pub fn edge_threshold_min(self) -> f32 {
        match self {
            Self::Low => 0.0833,
            Self::High => 0.0312,
        }
    }
}
