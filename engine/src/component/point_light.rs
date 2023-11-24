use egui::Color32;
use glm::Vec4;
use indextree::NodeId;
use reflect::{Reflect, ReflectDefault};
use utils::Component;

use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::render::Gizmos;
use crate::scene::Scene;

#[derive(Clone, Component, Reflect)]
#[reflect(Default, Component)]
#[reflect_attr(name = "Point Light")]
pub struct ComponentPointLight {
    pub active: bool,
    #[reflect_attr(min = 0.0)]
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
    fn draw_gizmos(&self, scene: &Scene, node: NodeId, gizmos: &mut Gizmos) {
        let transform = scene.get_world_transform(node);
        let color = self.color.to_normalized_gamma_f32();
        gizmos.set_color(&Vec4::from(color));
        gizmos.wire_sphere(&transform.position, self.radius);
    }
}
