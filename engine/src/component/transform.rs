use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::math::Transform;
use crate::render::Gizmos;
use crate::scene::Scene;
use glm::vec3;
use indextree::NodeId;
use reflect::Reflect;
use reflect::ReflectDefault;
use utils::Component;

#[derive(Default, Component, Reflect)]
#[reflect(Default, Component)]
pub struct ComponentTransform {
    pub transform: Transform,
}

impl Component for ComponentTransform {
    fn draw_gizmos(&self, scene: &Scene, node: NodeId, gizmos: &mut Gizmos) {
        let transform = scene.get_world_transform(node);
        gizmos.wire_sphere(&transform.position, 3.0);
        gizmos.wire_cube(&vec3(0.0, 0.0, 0.0), &vec3(10.0, 5.0, 3.0));
    }
}
