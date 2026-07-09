use crate as engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use egui::Color32;
use serde::{Deserialize, Serialize};

use super::{Component, ReflectComponent};

/// Directionless ambient light contribution.
#[derive(TypeUuid, Serialize, Component, Deserialize, Reflect)]
#[uuid = "545b3d95-8bad-4dbb-b1da-c649709f6235"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Ambient Light")]
#[serde(default)]
#[repr(C)]
pub struct ComponentAmbientLight {
    /// Whether the light contributes to scene lighting.
    pub active: bool,
    /// Ambient light color.
    pub color: Color32,
    /// Scalar intensity multiplier.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub intensity: f32,
}

impl Default for ComponentAmbientLight {
    fn default() -> Self {
        Self {
            active: true,
            color: Color32::WHITE,
            intensity: 0.15,
        }
    }
}

impl Component for ComponentAmbientLight {}
