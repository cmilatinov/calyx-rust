use egui::Rect;
use engine::component::{
    Component, ComponentCamera, ComponentEventContext, ComponentID, ComponentMesh,
    ComponentTransform, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
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

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "21b467c0-9409-4a7f-b750-418809609eb1"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Tank Controller")]
#[serde(default)]
#[repr(C)]
pub struct ComponentTankController {
    pub turret: GameObjectRef,
    pub barrel: GameObjectRef,
    pub camera: GameObjectRef,
    pub crosshair: GameObjectRef,
    pub move_speed: f32,
    pub reverse_speed: f32,
    pub hull_turn_speed: f32,
    pub turret_turn_speed: f32,
    pub projectile_speed: f32,
    pub projectile_lifetime: f32,
    pub projectile_radius: f32,
    pub muzzle_offset: f32,
    pub fire_cooldown: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub fire_cooldown_remaining: f32,
}

impl Default for ComponentTankController {
    fn default() -> Self {
        Self {
            turret: Default::default(),
            barrel: Default::default(),
            camera: Default::default(),
            crosshair: Default::default(),
            move_speed: 7.0,
            reverse_speed: 4.5,
            hull_turn_speed: 2.8,
            turret_turn_speed: 12.0,
            projectile_speed: 28.0,
            projectile_lifetime: 2.0,
            projectile_radius: 0.18,
            muzzle_offset: 1.0,
            fire_cooldown: 0.35,
            fire_cooldown_remaining: 0.0,
        }
    }
}

impl Component for ComponentTankController {}

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "951b5a6f-c3bb-46ed-b938-3909a2f439d1"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Projectile")]
#[serde(default)]
#[repr(C)]
pub struct ComponentProjectile {
    pub direction: Vec3,
    pub speed: f32,
    pub lifetime_remaining: f32,
    pub radius: f32,
}

impl Default for ComponentProjectile {
    fn default() -> Self {
        Self {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 28.0,
            lifetime_remaining: 2.0,
            radius: 0.18,
        }
    }
}

impl Component for ComponentProjectile {}

impl ComponentUpdate for ComponentProjectile {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_projectile(scene, game_object, resources.time().delta_time());
    }
}

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f2c31eb5-c999-4125-9cb0-a72a238eca2e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Projectile Target")]
#[serde(default)]
#[repr(C)]
pub struct ComponentProjectileTarget {
    pub hit_radius: f32,
    pub hit_count: u32,
}

impl Default for ComponentProjectileTarget {
    fn default() -> Self {
        Self {
            hit_radius: 1.0,
            hit_count: 0,
        }
    }
}

impl Component for ComponentProjectileTarget {}

impl ComponentUpdate for ComponentTankController {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let Some(mut controller) = scene
            .read_component::<ComponentTankController, _, _>(game_object, |component| *component)
        else {
            return;
        };

        let dt = resources.time().delta_time();
        let previous_tank_transform = scene.world_transform(game_object);
        let tank_transform = update_hull(scene, game_object, input, dt, &controller);
        let aim_point =
            cursor_ground_intersection(scene, input, &controller, tank_transform.position.y);
        update_crosshair(scene, &controller, aim_point);
        update_turret(scene, dt, &controller, &tank_transform, aim_point);
        update_shooting(scene, input, dt, &mut controller);
        update_camera(
            scene,
            &controller,
            &previous_tank_transform,
            &tank_transform,
        );
        let _ = scene.write_component::<ComponentTankController, _>(game_object, |component| {
            component.fire_cooldown_remaining = controller.fire_cooldown_remaining;
        });
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
    dt: TimeType,
    controller: &ComponentTankController,
    tank_transform: &Transform,
    aim_point: Option<Vec3>,
) {
    let Some(turret_object) = controller.turret.game_object(scene) else {
        return;
    };

    let Some(aim_point) = aim_point else {
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

fn update_crosshair(
    scene: &mut engine::scene::Scene,
    controller: &ComponentTankController,
    aim_point: Option<Vec3>,
) {
    let Some(crosshair_object) = controller.crosshair.game_object(scene) else {
        return;
    };
    let Some(aim_point) = aim_point else {
        return;
    };

    let mut transform = scene.world_transform(crosshair_object);
    transform.position = aim_point;
    scene.set_world_transform(crosshair_object, transform.matrix());
}

fn update_camera(
    scene: &mut engine::scene::Scene,
    controller: &ComponentTankController,
    previous_tank_transform: &Transform,
    tank_transform: &Transform,
) {
    let Some(camera_object) = controller.camera.game_object(scene) else {
        return;
    };

    let mut camera_transform = scene.world_transform(camera_object);
    follow_camera_xz(
        &mut camera_transform,
        previous_tank_transform,
        tank_transform,
    );
    scene.set_world_transform(camera_object, camera_transform.matrix());
}

fn update_shooting(
    scene: &mut engine::scene::Scene,
    input: &Input,
    dt: TimeType,
    controller: &mut ComponentTankController,
) {
    controller.fire_cooldown_remaining =
        advance_fire_cooldown(controller.fire_cooldown_remaining, dt);
    if !can_fire(
        input.action("shoot").pressed(),
        controller.fire_cooldown_remaining,
    ) {
        return;
    }

    if spawn_projectile(scene, controller) {
        controller.fire_cooldown_remaining = controller.fire_cooldown;
    }
}

fn advance_fire_cooldown(remaining: f32, dt: f32) -> f32 {
    (remaining - dt).max(0.0)
}

fn can_fire(shoot_pressed: bool, cooldown_remaining: f32) -> bool {
    shoot_pressed && cooldown_remaining <= f32::EPSILON
}

fn spawn_projectile(
    scene: &mut engine::scene::Scene,
    controller: &ComponentTankController,
) -> bool {
    let Some(barrel_object) = controller.barrel.game_object(scene) else {
        return false;
    };

    let barrel_transform = scene.world_transform(barrel_object);
    let direction = flatten_xz(barrel_transform.forward());
    if direction.magnitude_squared() <= f32::EPSILON {
        return false;
    }

    let visual = scene.read_component::<ComponentMesh, _, _>(barrel_object, |mesh| ComponentMesh {
        mesh: mesh.mesh.clone(),
        material: mesh.material.clone(),
    });
    let projectile_object = scene.create(
        Some(ComponentID {
            name: "Projectile".to_string(),
            ..Default::default()
        }),
        None,
    );
    let projectile_transform = Transform::from_components(
        barrel_transform.position + direction * controller.muzzle_offset,
        yaw_rotation(&direction),
        vec3(
            controller.projectile_radius * 2.0,
            controller.projectile_radius * 2.0,
            controller.projectile_radius * 2.0,
        ),
    );
    scene.set_world_transform(projectile_object, projectile_transform.matrix());
    scene.add_component(
        projectile_object,
        ComponentProjectile {
            direction,
            speed: controller.projectile_speed,
            lifetime_remaining: controller.projectile_lifetime,
            radius: controller.projectile_radius,
        },
    );
    if let Some(visual) = visual {
        scene.add_component(projectile_object, visual);
    }
    true
}

fn update_projectile(
    scene: &mut engine::scene::Scene,
    game_object: engine::scene::GameObject,
    dt: f32,
) {
    let Some(mut projectile) =
        scene.read_component::<ComponentProjectile, _, _>(game_object, |component| *component)
    else {
        return;
    };

    if projectile.lifetime_remaining <= f32::EPSILON {
        scene.delete(game_object);
        return;
    }

    let direction = flatten_xz(projectile.direction);
    if direction.magnitude_squared() <= f32::EPSILON {
        scene.delete(game_object);
        return;
    }

    let travel_time = projectile.lifetime_remaining.min(dt.max(0.0));
    let mut transform = scene.world_transform(game_object);
    let start = transform.position;
    let end = start + direction * projectile.speed * travel_time;

    if let Some(target) = first_projectile_hit(scene, game_object, start, end, projectile.radius) {
        let _ = scene.write_component::<ComponentProjectileTarget, _>(target, |target| {
            target.hit_count = target.hit_count.saturating_add(1);
        });
        scene.delete(game_object);
        return;
    }

    projectile.direction = direction;
    projectile.lifetime_remaining -= travel_time;
    if projectile.lifetime_remaining <= f32::EPSILON {
        scene.delete(game_object);
        return;
    }

    transform.position = end;
    scene.set_world_transform(game_object, transform.matrix());
    let _ = scene.write_component::<ComponentProjectile, _>(game_object, |component| {
        component.direction = projectile.direction;
        component.lifetime_remaining = projectile.lifetime_remaining;
    });
}

fn first_projectile_hit(
    scene: &engine::scene::Scene,
    projectile_object: engine::scene::GameObject,
    start: Vec3,
    end: Vec3,
    projectile_radius: f32,
) -> Option<engine::scene::GameObject> {
    scene.objects().find(|&candidate| {
        candidate != projectile_object
            && scene
                .read_component::<ComponentProjectileTarget, _, _>(candidate, |target| {
                    let target_transform = scene.world_transform(candidate);
                    segment_intersects_sphere(
                        start,
                        end,
                        target_transform.position,
                        projectile_radius + target.hit_radius,
                    )
                })
                .unwrap_or(false)
    })
}

fn segment_intersects_sphere(start: Vec3, end: Vec3, center: Vec3, radius: f32) -> bool {
    let segment = end - start;
    let segment_length_squared = segment.magnitude_squared();
    if segment_length_squared <= f32::EPSILON {
        return (start - center).magnitude_squared() <= radius * radius;
    }
    let t = ((center - start).dot(&segment) / segment_length_squared).clamp(0.0, 1.0);
    let closest = start + segment * t;
    (closest - center).magnitude_squared() <= radius * radius
}

fn follow_camera_xz(
    camera_transform: &mut Transform,
    previous_tank_transform: &Transform,
    tank_transform: &Transform,
) {
    let tank_delta = tank_transform.position - previous_tank_transform.position;
    camera_transform.position.x += tank_delta.x;
    camera_transform.position.z += tank_delta.z;
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

#[cfg(test)]
mod tests {
    use super::{
        advance_fire_cooldown, can_fire, clip_from_screen, flatten_xz, follow_camera_xz,
        screen_to_ground, segment_intersects_sphere, yaw_rotation,
    };
    use engine::component::{ComponentID, ComponentTransform};
    use engine::math::Transform;
    use engine::render::Camera;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::vec3;
    use serde_json::Value;

    const COMPONENT_ID_TYPE: &str = "02289c92-3412-406e-a7e5-3bbb15d7041e";
    const COMPONENT_TRANSFORM_TYPE: &str = "c5b3b71f-1f14-4b5b-9881-436118684d29";

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
    fn top_down_camera_center_ray_intersects_ground() {
        let tank_transform = Transform::from_xyz(0.0, 0.5, 0.0);
        let camera_transform = Transform::from_components(
            vec3(0.0, 16.0, -10.0),
            UnitQuaternion::from_euler_angles(std::f32::consts::FRAC_PI_3, 0.0, 0.0),
            vec3(1.0, 1.0, 1.0),
        );
        let camera_forward = camera_transform.forward();

        assert!(camera_forward.y < 0.0);
        assert!(camera_forward.z > 0.0);

        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1280.0, 720.0));
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

    #[test]
    fn sandbox_scene_camera_points_down_at_arena() {
        let scene: Value = serde_json::from_str(include_str!("../assets/scene.cxscene"))
            .expect("sandbox scene should be valid JSON");
        let components = scene["components"]
            .as_object()
            .expect("sandbox scene should contain component data");
        let camera_components = components
            .values()
            .find(|components| {
                components
                    .get(COMPONENT_ID_TYPE)
                    .and_then(|value| serde_json::from_value::<ComponentID>(value.clone()).ok())
                    .is_some_and(|id| id.name == "Camera")
            })
            .expect("sandbox scene should contain a Camera object");
        let camera_transform = serde_json::from_value::<ComponentTransform>(
            camera_components[COMPONENT_TRANSFORM_TYPE].clone(),
        )
        .expect("sandbox Camera should have a transform")
        .transform;
        let camera_forward = camera_transform.forward();

        assert!(camera_forward.y < -0.5);
        assert!(camera_forward.z > 0.45);
    }

    #[test]
    fn sandbox_scene_deserializes_objects() {
        let assets = engine::test_support::test_asset_context_with_assets(vec![
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
            std::env::current_dir().unwrap().join("assets"),
        ]);
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<engine::scene::Scene>(
                &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("assets")
                    .join("scene.cxscene"),
            )
            .expect("sandbox scene should load");

        let scene = scene_ref.read();
        let object_count = scene.objects().count();
        let named_object_count = scene
            .objects()
            .filter(|game_object| !scene.name(*game_object).is_empty())
            .count();

        assert!(
            object_count > 0,
            "sandbox scene should deserialize graph objects"
        );
        assert!(
            named_object_count > 0,
            "sandbox scene should deserialize ComponentID data"
        );

        let mut game = engine::context::GameContext::new(assets);
        game.scenes.load_scene(scene_ref.readonly());
        assert!(
            game.scenes.current_scene().objects().count() > 0,
            "scene manager should install deserialized scene objects"
        );
    }

    #[test]
    fn camera_follow_only_moves_xz_axes() {
        let rotation = UnitQuaternion::from_euler_angles(std::f32::consts::FRAC_PI_2, 0.2, 0.0);
        let mut camera_transform =
            Transform::from_components(vec3(2.0, 16.0, -8.0), rotation, vec3(1.0, 1.0, 1.0));
        let previous_tank_transform = Transform::from_xyz(1.0, 0.5, 2.0);
        let tank_transform = Transform::from_xyz(4.0, 5.0, 7.0);

        follow_camera_xz(
            &mut camera_transform,
            &previous_tank_transform,
            &tank_transform,
        );

        assert!((camera_transform.position.x - 5.0).abs() < 1e-6);
        assert!((camera_transform.position.y - 16.0).abs() < 1e-6);
        assert!((camera_transform.position.z - -3.0).abs() < 1e-6);
        assert_eq!(camera_transform.rotation, rotation);
    }

    #[test]
    fn fire_cooldown_counts_down_to_zero() {
        assert!((advance_fire_cooldown(0.35, 0.1) - 0.25).abs() < 1e-6);
        assert_eq!(advance_fire_cooldown(0.1, 0.35), 0.0);
    }

    #[test]
    fn fire_gate_requires_input_and_expired_cooldown() {
        assert!(can_fire(true, 0.0));
        assert!(!can_fire(false, 0.0));
        assert!(!can_fire(true, 0.1));
    }

    #[test]
    fn projectile_segment_hits_sphere_between_frames() {
        assert!(segment_intersects_sphere(
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 10.0),
            vec3(0.0, 0.0, 5.0),
            0.5,
        ));
        assert!(!segment_intersects_sphere(
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 10.0),
            vec3(2.0, 0.0, 5.0),
            0.5,
        ));
    }
}
