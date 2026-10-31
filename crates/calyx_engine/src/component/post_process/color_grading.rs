//! The colour grading effect component, read by the colour grading pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// White balance, contrast and saturation, after tone mapping.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "d2694fb5-4815-471e-9537-4ab502c580f0"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Colour Grading", category = "Post Processing")]
#[serde(default)]
pub struct ComponentColorGrading {
    /// Contrast around mid grey (linear 0.18): 1 leaves the image as it is,
    /// 0 flattens it to mid grey, above 1 spreads it out.
    #[reflect_attr(min = 0.0, max = 2.0, speed = 0.01)]
    pub contrast: f32,
    /// Saturation: 1 leaves the image as it is, 0 turns it grey.
    #[reflect_attr(min = 0.0, max = 2.0, speed = 0.01)]
    pub saturation: f32,
    /// White balance along the blue (negative) to orange (positive) axis.
    #[reflect_attr(min = -1.0, max = 1.0, speed = 0.01)]
    pub temperature: f32,
    /// White balance along the green (negative) to magenta (positive) axis.
    #[reflect_attr(min = -1.0, max = 1.0, speed = 0.01)]
    pub tint: f32,
}

impl Component for ComponentColorGrading {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentColorGrading {
    fn default() -> Self {
        Self {
            contrast: 1.0,
            saturation: 1.0,
            temperature: 0.0,
            tint: 0.0,
        }
    }
}
