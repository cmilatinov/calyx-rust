use crate as engine;
use crate::assets::skybox::Skybox;
use crate::assets::AssetRef;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

/// Environment-light component backed by a skybox asset.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "eb42f81f-45ab-428c-9d07-961696edc5fa"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Sky Light")]
#[serde(default)]
#[repr(C)]
pub struct ComponentSkyLight {
    /// Whether the sky light contributes to scene lighting.
    pub active: bool,
    /// Scalar intensity multiplier.
    pub intensity: f32,
    /// Skybox asset used for environment lighting and background rendering.
    pub skybox: AssetRef<Skybox>,
}

impl Default for ComponentSkyLight {
    fn default() -> Self {
        Self {
            active: true,
            intensity: 1.0,
            skybox: Default::default(),
        }
    }
}

impl Component for ComponentSkyLight {}
