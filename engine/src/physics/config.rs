use nalgebra_glm::Vec3;

pub struct PhysicsConfiguration {
    pub gravity: Vec3,
    pub physics_pipeline_active: bool,
    pub query_pipeline_active: bool,
}

impl Default for PhysicsConfiguration {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            physics_pipeline_active: true,
            query_pipeline_active: true,
        }
    }
}
