use crate as engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use egui::Color32;
use serde::{Deserialize, Serialize};

use super::{Component, ReflectComponent};

#[derive(TypeUuid, Serialize, Component, Deserialize, Reflect)]
#[uuid = "72b2568a-2ea0-4f58-ae76-e3f655006f0f"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Directional Light")]
#[serde(default)]
#[repr(C)]
pub struct ComponentDirectionalLight {
    pub active: bool,
    pub color: Color32,
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub intensity: f32,
}

impl Default for ComponentDirectionalLight {
    fn default() -> Self {
        Self {
            active: true,
            color: Color32::WHITE,
            intensity: 0.75,
        }
    }
}

impl Component for ComponentDirectionalLight {}
