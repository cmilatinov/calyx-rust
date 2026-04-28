use crate as engine;
use crate::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use crate::input::Input;
use crate::reflect::{Reflect, ReflectDefault};
use crate::resource::ResourceMap;
use crate::scene::GameObjectRef;
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Third Person Camera")]
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

impl Component for ComponentThirdPersonCamera {}

impl ComponentUpdate for ComponentThirdPersonCamera {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        if !scene.is_owner(game_object, resources.network()) {
            return;
        }
        let delta = input
            .input(|input| input.pointer.motion().unwrap_or_default())
            .unwrap_or(egui::Vec2::ZERO);
        let zoom_delta = input
            .input(|input| input.smooth_scroll_delta.y)
            .unwrap_or(0.0);

        let Some((target, sensitivity, zoom_sensitivity, distance, rotation)) =
            scene.read_component::<ComponentThirdPersonCamera, _, _>(game_object, |c| {
                (c.target, c.sensitivity, c.zoom_sensitivity, c.distance, c.rotation)
            })
        else {
            return;
        };

        let dt = resources.time().delta_time();
        let new_distance = distance - zoom_sensitivity * zoom_delta;
        let rot = Vec2::new(delta.x, delta.y).scale(dt * sensitivity);
        let mut new_rotation = rotation + rot;
        new_rotation.y =
            nalgebra::clamp(new_rotation.y, -89.0f32.to_radians(), 89.0f32.to_radians());

        scene.write_component::<ComponentThirdPersonCamera, _>(game_object, |c| {
            c.distance = new_distance;
            c.rotation = new_rotation;
        });

        let rotation_quat =
            UnitQuaternion::from_euler_angles(new_rotation.y, new_rotation.x, 0.0);
        let dir = rotation_quat * Vec3::z_axis();
        let pos = target
            .game_object(scene)
            .map(|go| scene.world_transform(go).position)
            .unwrap_or_default();
        let mut transform = scene.world_transform(game_object);
        transform.position = pos - new_distance * (*dir);
        transform.rotation = UnitQuaternion::face_towards(&dir, &Vec3::y_axis());
        scene.set_world_transform(game_object, transform.matrix());
    }
}
