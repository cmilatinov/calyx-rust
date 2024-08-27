use engine::component::{Component, ReflectComponent};
use engine::core::Time;
use engine::glm::{Vec2, Vec3};
use engine::input::Input;
use engine::nalgebra::UnitQuaternion;
use engine::reflect::{Reflect, ReflectDefault};
use engine::scene::{GameObject, GameObjectRef, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use engine::{nalgebra, serde_json};
use serde::{Deserialize, Serialize};

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Third Person Camera", update)]
pub struct ComponentThirdPersonCamera {
    pub target: Option<GameObjectRef>,
    pub sensitivity: f32,
    #[reflect_attr(min = 0.0)]
    pub distance: f32,
    #[serde(skip)]
    #[reflect_skip]
    rotation: Vec2,
}

impl Default for ComponentThirdPersonCamera {
    fn default() -> Self {
        Self {
            target: None,
            sensitivity: 0.5,
            distance: 5.0,
            rotation: Vec2::default(),
        }
    }
}

impl Component for ComponentThirdPersonCamera {
    fn update(&mut self, scene: &mut Scene, game_object: GameObject, input: &Input) {
        let (scroll_delta, delta) = input.input(|input| {
            (
                input.smooth_scroll_delta,
                input.pointer.motion().unwrap_or_default(),
            )
        });
        self.distance += scroll_delta.y;
        let rot = Vec2::new(delta.x, delta.y).scale(Time::delta_time() * self.sensitivity);
        self.rotation += rot;
        self.rotation.y =
            nalgebra::clamp(self.rotation.y, -89.0f32.to_radians(), 89.0f32.to_radians());
        let mut transform = scene.get_world_transform(game_object);
        let rotation = UnitQuaternion::from_euler_angles(self.rotation.y, self.rotation.x, 0.0);
        let dir = rotation * Vec3::z_axis();
        let pos = self
            .target
            .and_then(|t| t.game_object(scene))
            .map(|go| scene.get_world_transform(go).position)
            .unwrap_or_default();
        transform.position = pos - self.distance * (*dir);
        transform.rotation = UnitQuaternion::face_towards(&dir, &Vec3::y_axis());
        transform.update_matrix();
        scene.set_world_transform(game_object, transform);
    }
}
