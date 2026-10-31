//! The vignette effect component, read by the colour grading pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use serde::{Deserialize, Serialize};

/// Darkens, or tints, the edges of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "61efa204-ed0d-4d1f-86a4-b226be940b13"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Vignette", category = "Post Processing")]
#[serde(default)]
pub struct ComponentVignette {
    /// How far the vignette reaches in from the edges, from 0 (none) to 1.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub intensity: f32,
    /// How soft its falloff is, from sharp (near 0) to soft (1).
    #[reflect_attr(min = 0.01, max = 1.0, speed = 0.01)]
    pub smoothness: f32,
    /// The color the edges fade to.
    pub color: Color32,
}

impl Component for ComponentVignette {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentVignette {
    fn default() -> Self {
        Self {
            intensity: 0.4,
            smoothness: 0.2,
            color: Color32::BLACK,
        }
    }
}
