use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{GizmoIcon, Gizmos};
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::Vec4;
use serde::{Deserialize, Serialize};

/// Local point-light component.
///
/// The scene's enabled state switches it on and off; see
/// [`Scene::set_component_enabled`].
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5fd24d64-6661-40ba-94a5-4fca0d06ead1"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Point Light")]
#[serde(default)]
pub struct ComponentPointLight {
    /// Light influence radius.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub radius: f32,
    /// Light color.
    pub color: Color32,
    /// Radiance at the light. It falls off with the inverse square of the
    /// distance plus one, so a surface 1 unit away receives half of it, and
    /// fades smoothly to zero at `radius`.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub intensity: f32,
}

impl Default for ComponentPointLight {
    fn default() -> Self {
        Self {
            radius: 10.0,
            color: Color32::WHITE,
            intensity: 1.0,
        }
    }
}

impl Component for ComponentPointLight {
    /// Light gathering skips a disabled point light.
    fn can_disable(&self) -> bool {
        true
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        let color = self.color.to_normalized_gamma_f32();
        gizmos.set_color(&Vec4::from(color));
        gizmos.icon(GizmoIcon::Light, &transform.position, 0.75);
        gizmos.wire_sphere(&transform.position, self.radius);
    }
}
