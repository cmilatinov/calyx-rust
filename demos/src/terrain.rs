//! The terrain demo's rover: an object that drives a circle over a
//! `ComponentTerrain`, held on the ground by sampling it every frame.

use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentTerrain, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Drives its object around a circle in the world XZ plane, standing on the
/// highest enabled terrain under it and leaning with the ground's normal.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b1f3d7a2-6c4e-4f0b-9d8a-3e5c7a1b0472"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Terrain Rover")]
#[serde(default)]
pub struct ComponentTerrainRover {
    /// World X and Z of the circle's center.
    pub center: Vec2,
    /// Circle radius, in world units.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub radius: f32,
    /// Turning rate around the circle, in degrees per second.
    #[reflect_attr(speed = 0.1)]
    pub speed: f32,
    /// Starting angle on the circle, in degrees.
    #[reflect_attr(speed = 1.0)]
    pub phase: f32,
    /// Height of the object's origin above the ground.
    #[reflect_attr(speed = 0.01)]
    pub ground_offset: f32,
}

impl Default for ComponentTerrainRover {
    fn default() -> Self {
        Self {
            center: Vec2::zeros(),
            radius: 10.0,
            speed: 10.0,
            phase: 0.0,
            ground_offset: 0.5,
        }
    }
}

impl Component for ComponentTerrainRover {}

impl ComponentTerrainRover {
    /// World XZ position and travel direction on the circle at `time`.
    fn course(&self, time: f32) -> (Vec2, Vec3) {
        let angle = (self.phase + self.speed * time).to_radians();
        let (sin, cos) = angle.sin_cos();
        let position = self.center + vec2(cos, sin) * self.radius;
        let heading = vec3(-sin, 0.0, cos) * self.speed.signum();
        (position, heading)
    }
}

impl ComponentUpdate for ComponentTerrainRover {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let Some(rover) = this.read(scene, Clone::clone) else {
            return;
        };
        let (point, heading) = rover.course(resources.time().time() as f32);
        let Some(ground) = ComponentTerrain::sample_scene(scene, point) else {
            return;
        };
        let yaw = UnitQuaternion::face_towards(&heading, &Vec3::y());
        let lean = UnitQuaternion::rotation_between(&Vec3::y(), &ground.normal)
            .unwrap_or_else(UnitQuaternion::identity);
        let scale = scene.world_transform(game_object).scale;
        let transform = Transform::from_components(
            vec3(point.x, ground.height + rover.ground_offset, point.y),
            lean * yaw,
            scale,
        );
        scene.set_world_transform(game_object, transform.matrix());
    }
}
