use crate as calyx_engine;
use crate::assets::skybox::Skybox;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Environment-light component backed by a skybox asset.
///
/// The scene's enabled state switches it on and off; see
/// [`Scene::set_component_enabled`](crate::scene::Scene::set_component_enabled).
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "eb42f81f-45ab-428c-9d07-961696edc5fa"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Sky Light")]
#[serde(default)]
pub struct ComponentSkyLight {
    /// Scalar intensity multiplier.
    pub intensity: f32,
    /// Skybox asset used for environment lighting and background rendering.
    pub skybox: AssetRef<Skybox>,
}

impl Default for ComponentSkyLight {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            skybox: Default::default(),
        }
    }
}

impl Component for ComponentSkyLight {
    /// The scene renderer skips a disabled sky light, for both the sky it
    /// draws and the environment light it gives.
    fn can_disable(&self) -> bool {
        true
    }
}
