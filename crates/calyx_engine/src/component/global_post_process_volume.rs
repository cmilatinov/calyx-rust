use crate as calyx_engine;
use crate::assets::post_process_profile::PostProcessProfile;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Marks its game object as a scene-wide post-processing volume.
///
/// Its effects come from its `profile`, if it has one, overlaid with the
/// effect components on the same game object, such as
/// [`ComponentBloom`](super::ComponentBloom): an enabled component replaces
/// the profile's entry for its effect.
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
    /// The post-processing profile the volume starts from, or none.
    pub profile: AssetRef<PostProcessProfile>,
}

impl ComponentGlobalPostProcessVolume {
    /// A volume of `priority`.
    pub fn new(priority: i32) -> Self {
        Self {
            priority,
            profile: AssetRef::default(),
        }
    }
}

impl Component for ComponentGlobalPostProcessVolume {
    /// Post-process volume selection skips a disabled volume.
    fn can_disable(&self) -> bool {
        true
    }
}
