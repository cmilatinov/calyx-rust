use crate as engine;
use crate::resource::Resource;
use crate::utils::TypeUuid;
use nalgebra_glm::Vec3;

#[derive(Resource, TypeUuid)]
#[uuid = "290b91ee-503b-4609-a41c-e1570fbd6664"]
#[repr(C)]
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
