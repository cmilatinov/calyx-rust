use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::Gizmos;
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::Vec4;
use serde::{Deserialize, Serialize};

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5fd24d64-6661-40ba-94a5-4fca0d06ead1"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Point Light")]
#[serde(default)]
#[repr(C)]
pub struct ComponentPointLight {
    pub active: bool,
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub radius: f32,
    pub color: Color32,
}

impl Default for ComponentPointLight {
    fn default() -> Self {
        Self {
            active: true,
            radius: 10.0,
            color: Color32::WHITE,
        }
    }
}

impl Component for ComponentPointLight {
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.get_world_transform(game_object);
        let color = self.color.to_normalized_gamma_f32();
        gizmos.set_color(&Vec4::from(color));
        gizmos.wire_sphere(&transform.position, self.radius);
    }
}
