use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Marks its game object as a scene-wide post-processing volume.
///
/// The volume holds no settings: each effect is its own component on the
/// same game object, such as [`ComponentBloom`].
/// The renderer uses the enabled volume with the highest priority and the
/// enabled effect components beside it, and tone mapping alone when the
/// scene has no enabled volume. See
/// [`PostProcessSettings::resolve`](crate::render::postprocess::PostProcessSettings::resolve).
#[derive(Clone, Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "cdbfef3f-81a5-4ccb-a0f6-098bc5a26979"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Global Post Process Volume", category = "Post Processing")]
#[serde(default)]
pub struct ComponentGlobalPostProcessVolume {
    /// Among enabled volumes, the highest priority wins.
    pub priority: i32,
}

impl ComponentGlobalPostProcessVolume {
    /// A volume of `priority`.
    pub fn new(priority: i32) -> Self {
        Self { priority }
    }
}

impl Component for ComponentGlobalPostProcessVolume {
    /// Post-process volume selection skips a disabled volume.
    fn can_disable(&self) -> bool {
        true
    }
}
