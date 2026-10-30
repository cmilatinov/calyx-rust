use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::postprocess::PostProcessSettings;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Scene-wide post-processing settings.
///
/// The renderer uses the enabled volume with the highest priority, and
/// [`PostProcessSettings::default`] when the scene has none.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "cdbfef3f-81a5-4ccb-a0f6-098bc5a26979"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Global Post Process Volume")]
#[serde(default)]
pub struct ComponentGlobalPostProcessVolume {
    /// Among enabled volumes, the highest priority wins.
    pub priority: i32,
    /// The settings this volume applies.
    pub settings: PostProcessSettings,
}

impl Default for ComponentGlobalPostProcessVolume {
    fn default() -> Self {
        Self::new(PostProcessSettings::default())
    }
}

impl ComponentGlobalPostProcessVolume {
    /// A volume of priority zero applying `settings`.
    pub fn new(settings: PostProcessSettings) -> Self {
        Self {
            priority: 0,
            settings,
        }
    }
}

impl Component for ComponentGlobalPostProcessVolume {}
