use crate as calyx_engine;
use crate::{
    reflect::{Reflect, ReflectDefault},
    utils::{ReflectTypeUuidDynamic, TypeUuid},
};
use egui::Color32;
use serde::{Deserialize, Serialize};

use super::{Component, ReflectComponent};

/// Infinite directional light component.
///
/// The scene's enabled state switches it on and off; see
/// [`Scene::set_component_enabled`](crate::scene::Scene::set_component_enabled).
#[derive(TypeUuid, Serialize, Component, Deserialize, Reflect)]
#[uuid = "72b2568a-2ea0-4f58-ae76-e3f655006f0f"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Directional Light")]
#[serde(default)]
pub struct ComponentDirectionalLight {
    /// Light color.
    pub color: Color32,
    /// Scalar intensity multiplier.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub intensity: f32,
    /// Whether the light casts shadows. Only the first enabled directional
    /// light that casts shadows gets them; the others light unoccluded.
    pub cast_shadows: bool,
    /// View depth from the camera, in world units, that the shadow cascades
    /// cover. Past it the light falls unoccluded.
    #[reflect_attr(min = 1.0, speed = 0.5)]
    pub shadow_distance: f32,
    /// Slope-scaled and constant depth bias of the shadow pass, in units of
    /// the default. Raise it against shadow acne, lower it against light
    /// leaking under casters.
    #[reflect_attr(min = 0.0, max = 10.0, speed = 0.05)]
    pub depth_bias: f32,
    /// How far, in shadow texels, a shaded point moves along its normal
    /// before it looks up its shadow.
    #[reflect_attr(min = 0.0, max = 10.0, speed = 0.05)]
    pub normal_bias: f32,
}

impl Default for ComponentDirectionalLight {
    fn default() -> Self {
        Self {
            color: Color32::WHITE,
            intensity: 0.75,
            cast_shadows: false,
            shadow_distance: 50.0,
            depth_bias: 1.0,
            normal_bias: 1.0,
        }
    }
}

impl Component for ComponentDirectionalLight {
    /// Light gathering skips a disabled directional light.
    fn can_disable(&self) -> bool {
        true
    }
}
