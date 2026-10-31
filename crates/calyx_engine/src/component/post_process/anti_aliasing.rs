//! The anti-aliasing effect component, read by the FXAA pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Screen-space anti-aliasing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "c19ff2b7-c3ed-4f04-9dfa-e026efdedf82"]
#[reflect(Default, TypeUuidDynamic, Component)]
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
        }
    }
}
