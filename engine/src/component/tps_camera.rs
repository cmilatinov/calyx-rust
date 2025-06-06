use crate as engine;
use crate::component::{Component, ComponentEventContext, ReflectComponent};
use crate::input::Input;
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::scene::GameObjectRef;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Third Person Camera", update)]
#[repr(C)]
pub struct ComponentThirdPersonCamera {
    pub target: GameObjectRef,
    #[reflect_attr(speed = 0.01, min = 0.0)]
    pub sensitivity: f32,
    #[reflect_attr(speed = 0.01, min = 0.0)]
    pub zoom_sensitivity: f32,
    #[reflect_attr(min = 0.0)]
    pub distance: f32,
    #[serde(skip)]
    #[reflect_skip]
    rotation: Vec2,
}

impl Default for ComponentThirdPersonCamera {
    fn default() -> Self {
        Self {
            target: Default::default(),
            sensitivity: 0.5,
            zoom_sensitivity: 0.25,
            distance: 5.0,
            rotation: Default::default(),
        }
    }
}

impl Component for ComponentThirdPersonCamera {
    fn update(
        &mut self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let delta = input
            .input(|input| input.pointer.motion().unwrap_or_default())
            .unwrap_or(egui::Vec2::ZERO);
        let zoom_delta = input
            .input(|input| input.smooth_scroll_delta.y)
            .unwrap_or(0.0);
        self.distance -= self.zoom_sensitivity * zoom_delta;
        let rot =
            Vec2::new(delta.x, delta.y).scale(resources.time().delta_time() * self.sensitivity);
        self.rotation += rot;
        self.rotation.y =
            nalgebra::clamp(self.rotation.y, -89.0f32.to_radians(), 89.0f32.to_radians());
        let mut transform = scene.get_world_transform(game_object);
        let rotation = UnitQuaternion::from_euler_angles(self.rotation.y, self.rotation.x, 0.0);
        let dir = rotation * Vec3::z_axis();
        let pos = self
            .target
            .game_object(scene)
            .map(|go| scene.get_world_transform(go).position)
            .unwrap_or_default();
        transform.position = pos - self.distance * (*dir);
        transform.rotation = UnitQuaternion::face_towards(&dir, &Vec3::y_axis());
        transform.update_matrix();
        scene.set_world_transform(game_object, transform.matrix);
    }
}
