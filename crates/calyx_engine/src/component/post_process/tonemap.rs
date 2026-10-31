//! The tone-mapping effect component, read by the tone-map pass.

use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Tone-mapping curve and exposure, as an effect component on a
/// post-process volume's game object.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TypeUuid, Component, Reflect)]
#[uuid = "68ac12ac-fc9a-489e-b305-363eaf593966"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Tone Mapping", category = "Post Processing")]
#[serde(default)]
pub struct ComponentTonemap {
    /// Curve that maps linear HDR color to the display range.
    pub preset: TonemappingPreset,
    /// Exposure in stops: the scene is scaled by `2^exposure_ev` before the
    /// curve.
    #[reflect_attr(speed = 0.05)]
    pub exposure_ev: f32,
}

impl Default for ComponentTonemap {
    fn default() -> Self {
        Self {
            preset: TonemappingPreset::Aces,
            exposure_ev: 0.0,
        }
    }
}

impl Component for ComponentTonemap {
    /// A disabled tone-mapping component leaves the defaults in effect.
    fn can_disable(&self) -> bool {
        true
    }
}

impl ComponentTonemap {
    /// The linear factor the scene is scaled by before the curve.
    pub fn exposure_scale(&self) -> f32 {
        self.exposure_ev.exp2()
    }
}

/// Tone-mapping curves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TypeUuid, Reflect)]
#[uuid = "a4d6b439-bdc2-4aeb-aaf7-1f14f147255b"]
pub enum TonemappingPreset {
    /// Stephen Hill's fit of the ACES reference rendering and output
    /// transforms.
    #[default]
    Aces,
}

impl TonemappingPreset {
    /// The value the tone-map shader switches on.
    pub(crate) fn shader_index(self) -> u32 {
        match self {
            Self::Aces => 0,
        }
    }
}
