//! The depth of field effect component, read by the depth of field pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Depth of field: the scene blurs with distance from a focus plane.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "ac9e495b-1859-42d8-b1c5-0d35f113c089"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Depth Of Field", category = "Post Processing")]
#[serde(default)]
pub struct ComponentDepthOfField {
    /// View distance, in world units, that is sharp.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub focus_distance: f32,
    /// Distance in front of and behind the focus plane, in world units, over
    /// which the blur grows from none to `max_blur`.
    #[reflect_attr(min = 0.01, speed = 0.05)]
    pub focus_range: f32,
    /// Largest blur radius, in pixels.
    #[reflect_attr(min = 0.0, max = 32.0, speed = 0.1)]
    pub max_blur: f32,
}

impl ComponentDepthOfField {
    /// The largest `max_blur` the pass honors, in pixels. The gather's tap
    /// count grows with the radius, so it is capped.
    pub const MAX_BLUR_LIMIT: f32 = 32.0;

    /// The blur radius the pass uses: `max_blur` within
    /// `[0, MAX_BLUR_LIMIT]`.
    pub fn blur_radius(&self) -> f32 {
        self.max_blur.clamp(0.0, Self::MAX_BLUR_LIMIT)
    }

    /// Spacing of the gather's spiral. Taps number about
    /// `radius^2 / (2 * spacing)`, so the spacing grows past 8 pixels to
    /// keep the count at most about 256.
    pub(crate) fn radius_scale(&self) -> f32 {
        (self.blur_radius() / 16.0).max(0.5)
    }
}

impl Component for ComponentDepthOfField {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentDepthOfField {
    fn default() -> Self {
        Self {
            focus_distance: 10.0,
            focus_range: 10.0,
            max_blur: 8.0,
        }
    }
}
