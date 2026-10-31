//! The ambient occlusion effect component, read by the SSAO pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Screen-space ambient occlusion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "01a950bc-d431-4495-8b86-7d51c5f2b4b0"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Ambient Occlusion", category = "Post Processing")]
#[serde(default)]
pub struct ComponentAmbientOcclusion {
    /// Radius of the hemisphere tested around each pixel, in world units.
    #[reflect_attr(min = 0.01, speed = 0.01)]
    pub radius: f32,
    /// Strength of the darkening: the unoccluded fraction is raised to this
    /// power. Zero turns the effect off.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub intensity: f32,
    /// Depth difference, in world units, below which a sample does not count
    /// as occluded. Raise it if flat surfaces show speckles.
    #[reflect_attr(min = 0.0, speed = 0.001)]
    pub bias: f32,
    /// Samples tested per pixel. Clamped to `1..=64`.
    pub sample_count: u32,
}

impl Component for ComponentAmbientOcclusion {
    /// A disabled effect component switches its pass off.
    fn can_disable(&self) -> bool {
        true
    }
}

impl Default for ComponentAmbientOcclusion {
    fn default() -> Self {
        Self {
            radius: 0.5,
            intensity: 1.0,
            bias: 0.025,
            sample_count: 16,
        }
    }
}

impl ComponentAmbientOcclusion {
    /// Most samples `sample_count` selects.
    pub const MAX_SAMPLES: u32 = 64;

    /// `sample_count`, clamped to `1..=MAX_SAMPLES`.
    pub fn samples(&self) -> u32 {
        self.sample_count.clamp(1, Self::MAX_SAMPLES)
    }
}
