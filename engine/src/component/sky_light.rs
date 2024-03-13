use serde::{Deserialize, Serialize};

use crate as engine;
use crate::assets::skybox::Skybox;
use crate::component::{Component, ReflectComponent};
use crate::core::Ref;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "eb42f81f-45ab-428c-9d07-961696edc5fa"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Sky Light")]
#[serde(default)]
pub struct ComponentSkyLight {
    pub active: bool,
    pub intensity: f32,
    pub skybox: Option<Ref<Skybox>>,
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
