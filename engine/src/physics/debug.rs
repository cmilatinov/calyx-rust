use crate::render::Gizmos;
use rapier3d::math::{Point, Real};
use rapier3d::pipeline::{DebugRenderBackend, DebugRenderObject};

pub struct PhysicsDebugRenderer<'a> {
    gizmos: Gizmos<'a>,
}

impl<'a> From<Gizmos<'a>> for PhysicsDebugRenderer<'a> {
    fn from(value: Gizmos<'a>) -> Self {
        Self { gizmos: value }
    }
}

impl<'a> DebugRenderBackend for PhysicsDebugRenderer<'a> {
    fn draw_line(
        &mut self,
        _object: DebugRenderObject,
        a: Point<Real>,
        b: Point<Real>,
        color: [f32; 4],
    ) {
        let a = glm::vec3(a.x, a.y, a.z);
        let b = glm::vec3(b.x, b.y, b.z);
        self.gizmos.set_color(&color.into());
        self.gizmos.line(&a, &b);
    }
}
