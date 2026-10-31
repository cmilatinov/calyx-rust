use crate as calyx_engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use egui::Color32;
use serde::{Deserialize, Serialize};

use super::{Component, ReflectComponent};

/// Directionless ambient light contribution.
///
/// The scene's enabled state switches it on and off; see
/// [`Scene::set_component_enabled`](crate::scene::Scene::set_component_enabled).
#[derive(TypeUuid, Serialize, Component, Deserialize, Reflect)]
#[uuid = "545b3d95-8bad-4dbb-b1da-c649709f6235"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Ambient Light")]
#[serde(default)]
pub struct ComponentAmbientLight {
    /// Ambient light color.
    pub color: Color32,
    /// Scalar intensity multiplier.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub intensity: f32,
}

impl Default for ComponentAmbientLight {
    fn default() -> Self {
        Self {
            color: Color32::WHITE,
            intensity: 0.15,
        }
    }
}

impl Component for ComponentAmbientLight {
    /// Light gathering skips a disabled ambient light.
    fn can_disable(&self) -> bool {
        true
    }
}
