use engine::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use engine::core::TimeType;
use engine::input::Input;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::GameObjectRef;
use engine::try_all;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8c4d2976-47c1-403c-a248-6db84a124816"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Player Controller")]
#[repr(C)]
pub struct ComponentPlayerController {
    pub camera: GameObjectRef,
    pub move_speed: f32,
    pub sprint_multiplier: f32,
    pub rotation_speed: f32,
}

impl Default for ComponentPlayerController {
    fn default() -> Self {
        Self {
            camera: Default::default(),
            move_speed: 5.0,
            sprint_multiplier: 2.0,
            rotation_speed: 2.0,
        }
    }
}

impl Component for ComponentPlayerController {}

impl ComponentUpdate for ComponentPlayerController {
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

        let Some((camera_ref, move_speed, sprint_multiplier)) =
            scene.read_component::<ComponentPlayerController, _, _>(game_object, |c| {
                (c.camera, c.move_speed, c.sprint_multiplier)
            })
        else {
            return;
        };

        let dt = resources.time().delta_time();
        update_movement(
            scene,
            game_object,
            input,
            dt,
            camera_ref,
            move_speed,
            sprint_multiplier,
        );
    }
}

fn update_movement(
    scene: &mut engine::scene::Scene,
    game_object: engine::scene::GameObject,
    input: &Input,
    dt: TimeType,
    camera_ref: GameObjectRef,
    move_speed: f32,
    sprint_multiplier: f32,
) {
    use egui::Key;

    let forward = input
        .input(|i| (i.key_down(Key::W) as i32 - i.key_down(Key::S) as i32) as f32)
        .unwrap_or_default();
    let right = input
        .input(|i| (i.key_down(Key::D) as i32 - i.key_down(Key::A) as i32) as f32)
        .unwrap_or_default();
    let run = input.input(|i| i.modifiers.shift).unwrap_or(false);
    let speed = move_speed * if run { sprint_multiplier } else { 1.0 } * dt;

    try_all!(
        None => return;
        let camera = camera_ref.game_object(scene);
    );
    let camera_transform = scene.world_transform(camera);
    let camera_forward = camera_transform.forward().xz().normalize();
    let camera_right = camera_transform.right().xz().normalize();
    let move_direction = camera_forward * forward + camera_right * right;
    let move_vector = Vec3::new(move_direction.x, 0.0, move_direction.y) * speed;

    if move_direction.metric_distance(&Vec2::zeros()) <= f32::EPSILON {
        return;
    }

    let mut transform = scene.world_transform(game_object);
    transform.position += move_vector;

    if let Some(move_dir_2d) = move_direction.try_normalize(f32::EPSILON) {
        transform.rotation = UnitQuaternion::face_towards(
            &Vec3::new(move_dir_2d.x, 0.0, move_dir_2d.y),
            &Vec3::y_axis(),
        );
    }
    scene.set_world_transform(game_object, transform.matrix());
}

