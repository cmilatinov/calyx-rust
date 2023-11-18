use egui::Color32;
use indextree::NodeId;

use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::render::Gizmos;
use crate::scene::Scene;

#[derive(Clone, Component, Reflect)]
#[reflect(Default, Component)]
#[reflect_attr(name = "Camera")]
pub struct ComponentCamera {
    pub fov: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub clear_color: Color32,
}

impl Default for ComponentCamera {
    fn default() -> Self {
        Self {
            fov: 70.0,
            near_plane: 0.1,
            far_plane: 100.0,
            clear_color: Color32::BLACK,
        }
    }
}

impl Component for ComponentCamera {
    fn draw_gizmos(&self, scene: &Scene, node: NodeId, gizmos: &mut Gizmos) {
        let transform = scene.get_world_transform(node);
        gizmos.wire_frustum(
            &transform,
            16.0 / 9.0,
            self.fov,
            self.near_plane,
            self.far_plane,
        );
    }
}
