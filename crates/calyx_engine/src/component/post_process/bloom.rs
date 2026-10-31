//! The bloom effect component, read by the bloom pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Bloom: light above a threshold is blurred and added back onto the scene.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "7e09858d-fd71-4729-bb90-8ed76c0bda9a"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Bloom", category = "Post Processing")]
#[serde(default)]
pub struct ComponentBloom {
    /// Linear HDR brightness, of a pixel's brightest channel, above which
    /// light blooms.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub threshold: f32,
    /// Softness of the threshold, as a fraction of it: light starts to bloom
    /// at `threshold * (1 - knee)` and blooms fully from
    /// `threshold * (1 + knee)`. Zero is a hard cut.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub knee: f32,
    /// How much of the blurred light is added back onto the scene.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub intensity: f32,
    /// How many half-size mips the light is blurred through. Each one doubles
    /// how far the glow spreads. Clamped to `1..=8`.
    pub radius: u32,
}

impl Component for ComponentBloom {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentBloom {
    fn default() -> Self {
        Self {
            threshold: 1.0,
            knee: 0.5,
            intensity: 0.5,
            radius: 6,
        }
    }
}

impl ComponentBloom {
    /// Most mips `radius` selects.
    pub const MAX_RADIUS: u32 = 8;

    /// How many mips bloom blurs through at `size`: `radius`, clamped
    /// to `1..=MAX_RADIUS` and to the halvings `size` allows before a side
    /// reaches one pixel.
    pub fn mip_count(&self, size: (u32, u32)) -> usize {
        let halvings = size.0.min(size.1).max(2).ilog2();
        self.radius.clamp(1, Self::MAX_RADIUS).min(halvings) as usize
    }
}
