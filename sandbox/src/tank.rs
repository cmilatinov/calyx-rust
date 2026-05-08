use egui::Rect;
use engine::component::{
    Component, ComponentCamera, ComponentEventContext, ComponentTransform, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use engine::core::TimeType;
use engine::input::Input;
use engine::math::Transform;
use engine::reflect::{Reflect, ReflectDefault};
use engine::render::Camera;
use engine::resource::ResourceMap;
use engine::scene::GameObjectRef;
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, vec4, Mat4, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::ReflectRegistrationFn;

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "21b467c0-9409-4a7f-b750-418809609eb1"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Tank Controller")]
#[serde(default)]
#[repr(C)]
pub struct ComponentTankController {
    pub turret: GameObjectRef,
    pub camera: GameObjectRef,
    pub move_speed: f32,
    pub reverse_speed: f32,
    pub hull_turn_speed: f32,
    pub turret_turn_speed: f32,
    pub camera_height: f32,
    pub camera_distance: f32,
    pub camera_smoothing: f32,
}

impl Default for ComponentTankController {
    fn default() -> Self {
        Self {
            turret: Default::default(),
            camera: Default::default(),
            move_speed: 7.0,
            reverse_speed: 4.5,
            hull_turn_speed: 2.8,
            turret_turn_speed: 12.0,
            camera_height: 16.0,
            camera_distance: 10.0,
            camera_smoothing: 10.0,
        }
    }
}

impl Component for ComponentTankController {}

impl ComponentUpdate for ComponentTankController {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let Some(controller) = scene
            .read_component::<ComponentTankController, _, _>(game_object, |component| *component)
        else {
            return;
        };

        let dt = resources.time().delta_time();
        let tank_transform = update_hull(scene, game_object, input, dt, &controller);
        let camera_target = desired_camera_transform(&tank_transform, &controller);
        update_turret(scene, input, dt, &controller, &tank_transform);
        update_camera(scene, dt, &controller, &tank_transform, &camera_target);
    }
}

fn update_hull(
    scene: &mut engine::scene::Scene,
    game_object: engine::scene::GameObject,
    input: &Input,
    dt: TimeType,
    controller: &ComponentTankController,
) -> Transform {
    let mut transform = scene.world_transform(game_object);
    let throttle = input.axis("move_forward");
    let steer = input.axis("move_right");

    if steer.abs() > f32::EPSILON {
        transform.rotate(&UnitQuaternion::from_euler_angles(
            0.0,
            steer * controller.hull_turn_speed * dt,
            0.0,
        ));
    }

    if throttle.abs() > f32::EPSILON {
        let speed = if throttle >= 0.0 {
            controller.move_speed
        } else {
            controller.reverse_speed
        };
        let forward = flatten_xz(transform.forward());
        transform.translate(&(forward * (throttle * speed * dt)));
    }

    scene.set_world_transform(game_object, transform.matrix());
    scene.world_transform(game_object)
}

fn update_turret(
    scene: &mut engine::scene::Scene,
    input: &Input,
    dt: TimeType,
    controller: &ComponentTankController,
    tank_transform: &Transform,
) {
    let Some(turret_object) = controller.turret.game_object(scene) else {
        return;
    };

    let Some(aim_point) =
        cursor_ground_intersection(scene, input, controller, tank_transform.position.y)
    else {
        return;
    };

    let aim_direction = flatten_xz(aim_point - tank_transform.position);
    if aim_direction.magnitude_squared() <= f32::EPSILON {
        return;
    }

    let desired_world_rotation = yaw_rotation(&aim_direction);
    let desired_local_rotation = tank_transform.rotation.inverse() * desired_world_rotation;
    let blend = (controller.turret_turn_speed * dt).clamp(0.0, 1.0);

    let _ = scene.write_component::<ComponentTransform, _>(turret_object, |transform| {
        transform.transform.rotation = transform
            .transform
            .rotation
            .slerp(&desired_local_rotation, blend);
    });
}

fn update_camera(
    scene: &mut engine::scene::Scene,
    dt: TimeType,
    controller: &ComponentTankController,
    tank_transform: &Transform,
    desired_transform: &Transform,
) {
    let Some(camera_object) = controller.camera.game_object(scene) else {
        return;
    };

    let mut camera_transform = scene.world_transform(camera_object);
    let blend = (controller.camera_smoothing * dt).clamp(0.0, 1.0);
    camera_transform.position += (desired_transform.position - camera_transform.position) * blend;
    face_towards(&mut camera_transform, &(tank_transform.position + vec3(0.0, 0.75, 0.0)));
    scene.set_world_transform(camera_object, camera_transform.matrix());
}

fn desired_camera_transform(
    tank_transform: &Transform,
    controller: &ComponentTankController,
) -> Transform {
    let mut transform = Transform::from_xyz(
        tank_transform.position.x,
        tank_transform.position.y + controller.camera_height,
        tank_transform.position.z - controller.camera_distance,
    );
    face_towards(&mut transform, &(tank_transform.position + vec3(0.0, 0.75, 0.0)));
    transform
}

fn face_towards(transform: &mut Transform, target: &Vec3) {
    let direction = target - transform.position;
    if direction.magnitude_squared() <= f32::EPSILON {
        return;
    }
    transform.rotation = UnitQuaternion::face_towards(&direction.normalize(), &Vec3::y_axis());
}

fn cursor_ground_intersection(
    scene: &engine::scene::Scene,
    input: &Input,
    controller: &ComponentTankController,
    ground_y: f32,
) -> Option<Vec3> {
    let rect = input.res().map(|response| response.rect)?;
    let cursor = input
        .input(|state| state.pointer.interact_pos())
        .flatten()?;
    let camera_object = controller.camera.game_object(scene)?;
    let camera_transform = scene.world_transform(camera_object);
    let aspect = rect.width() / rect.height().max(f32::EPSILON);
    let camera = scene
        .read_component::<ComponentCamera, _, _>(camera_object, |component| {
            Camera::new(
                aspect,
                component.fov,
                component.near_plane,
                component.far_plane,
            )
        })
        .unwrap_or_else(|| Camera::new(aspect, 70.0f32.to_radians(), 0.1, 1000.0));
    screen_to_ground(&camera_transform, &camera, rect, cursor, ground_y)
}

fn screen_to_ground(
    camera_transform: &Transform,
    camera: &Camera,
    rect: Rect,
    cursor: egui::Pos2,
    ground_y: f32,
) -> Option<Vec3> {
    let clip = clip_from_screen(rect, cursor);
    let inv_view_projection = inverse_view_projection(camera_transform, camera)?;

    let near = world_from_clip(&inv_view_projection, vec4(clip.x, clip.y, 0.0, 1.0))?;
    let far = world_from_clip(&inv_view_projection, vec4(clip.x, clip.y, 1.0, 1.0))?;
    let direction = far - near;
    if direction.y.abs() <= f32::EPSILON {
        return None;
    }

    let t = (ground_y - near.y) / direction.y;
    (t >= 0.0).then_some(near + direction * t)
}

fn clip_from_screen(rect: Rect, cursor: egui::Pos2) -> egui::Vec2 {
    egui::vec2(
        ((cursor.x - rect.left()) / rect.width()) * 2.0 - 1.0,
        1.0 - ((cursor.y - rect.top()) / rect.height()) * 2.0,
    )
}

fn inverse_view_projection(camera_transform: &Transform, camera: &Camera) -> Option<Mat4> {
    let view_projection = camera.projection * camera_transform.inverse_matrix();
    view_projection.try_inverse()
}

fn world_from_clip(inverse_view_projection: &Mat4, clip: Vec4) -> Option<Vec3> {
    let world = inverse_view_projection * clip;
    (world.w.abs() > f32::EPSILON).then_some(vec3(
        world.x / world.w,
        world.y / world.w,
        world.z / world.w,
    ))
}

fn flatten_xz(vector: Vec3) -> Vec3 {
    let flattened = vec3(vector.x, 0.0, vector.z);
    if flattened.magnitude_squared() <= f32::EPSILON {
        Vec3::zeros()
    } else {
        flattened.normalize()
    }
}

fn yaw_rotation(direction: &Vec3) -> UnitQuaternion<f32> {
    UnitQuaternion::from_euler_angles(0.0, direction.x.atan2(direction.z), 0.0)
}

inventory::submit! {
    ReflectRegistrationFn {
        name: "ComponentTankController",
        function: |registry| {
            registry.register::<ComponentTankController>();
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clip_from_screen, desired_camera_transform, flatten_xz, screen_to_ground, yaw_rotation,
        ComponentTankController,
    };
    use engine::math::Transform;
    use engine::render::Camera;
    use nalgebra_glm::vec3;

    #[test]
    fn yaw_rotation_faces_positive_x() {
        let rotation = yaw_rotation(&vec3(1.0, 0.0, 0.0));
        let forward = rotation * vec3(0.0, 0.0, 1.0);
        assert!((forward.x - 1.0).abs() < 1e-4);
        assert!(forward.z.abs() < 1e-4);
    }

    #[test]
    fn clip_space_maps_view_center_to_origin() {
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(500.0, 250.0));
        let clip = clip_from_screen(rect, rect.center());
        assert!(clip.x.abs() < 1e-6);
        assert!(clip.y.abs() < 1e-6);
    }

    #[test]
    fn flatten_xz_discards_vertical_component() {
        let flattened = flatten_xz(vec3(3.0, 9.0, 4.0));
        assert!(flattened.y.abs() < 1e-6);
        assert!((flattened.x - 0.6).abs() < 1e-4);
        assert!((flattened.z - 0.8).abs() < 1e-4);
    }

    #[test]
    fn desired_camera_faces_down_toward_tank() {
        let tank_transform = Transform::from_xyz(0.0, 0.5, 0.0);
        let controller = ComponentTankController::default();
        let camera_transform = desired_camera_transform(&tank_transform, &controller);
        let target = tank_transform.position + vec3(0.0, 0.75, 0.0);
        let target_direction = (target - camera_transform.position).normalize();
        let camera_forward = camera_transform.forward().normalize();

        assert!(camera_forward.y < 0.0);
        assert!(camera_forward.dot(&target_direction) > 0.999);

        let rect =
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1280.0, 720.0));
        let camera = Camera::new(rect.aspect_ratio(), 70.0f32.to_radians(), 0.1, 1000.0);
        let hit = screen_to_ground(
            &camera_transform,
            &camera,
            rect,
            rect.center(),
            tank_transform.position.y,
        )
        .expect("center ray should intersect the tank ground plane");

        assert!((hit.x - tank_transform.position.x).abs() < 1e-3);
        assert!((hit.y - tank_transform.position.y).abs() < 1e-3);
    }
}
