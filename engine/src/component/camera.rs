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
#[uuid = "a85867d2-3e68-42b2-b943-ea78c7c6ddb5"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Camera")]
#[serde(default)]
#[repr(C)]
pub struct ComponentCamera {
    #[reflect_attr(angle, min = 30.0, max = 160.0, speed = 0.1)]
    pub fov: f32,
    #[reflect_attr(min = 0.01, speed = 0.01)]
    pub near_plane: f32,
    #[reflect_attr(min = 20.0, max = 1000.0, speed = 1.0)]
    pub far_plane: f32,
    pub clear_color: Color32,
    pub enabled: bool,
}

impl Default for ComponentCamera {
    fn default() -> Self {
        Self {
            fov: 70.0f32.to_radians(),
            near_plane: 0.1,
            far_plane: 100.0,
            clear_color: Color32::BLACK,
            enabled: true,
        }
    }
}

impl Component for ComponentCamera {
    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.get_world_transform(game_object);
        gizmos.set_color(&Vec4::new(1.0, 1.0, 1.0, 1.0));
        gizmos.wire_frustum(
            &transform,
            16.0 / 9.0,
            self.fov,
            self.near_plane,
            self.far_plane,
        );
    }
}
