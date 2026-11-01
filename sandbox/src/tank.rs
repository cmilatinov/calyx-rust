use crate::combat::{record_kill, KillEvent};
use crate::spawn::ComponentRespawnState;
use calyx_engine::component::{
    ColliderShape, Component, ComponentCamera, ComponentCollider, ComponentEventContext,
    ComponentID, ComponentMesh, ComponentRigidBody, ComponentTransform, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::core::TimeType;
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::render::{Camera, Ray};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::GameObjectRef;
use calyx_engine::scene::Scene;
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::Entity;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec3};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, RigidBodyType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const TANK_COLLIDER_HALF_EXTENTS: Vec3 = Vec3::new(0.75, 0.5, 1.0);
const TANK_COLLIDER_OFFSET: Vec3 = Vec3::new(0.0, 0.75, 0.0);
const COLLISION_SKIN: f32 = 0.01;

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "21b467c0-9409-4a7f-b750-418809609eb1"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Tank Controller")]
#[serde(default)]
pub struct ComponentTankController {
    pub turret: GameObjectRef,
    pub barrel: GameObjectRef,
    pub camera: GameObjectRef,
    pub crosshair: GameObjectRef,
    /// Top forward speed, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub move_speed: f32,
    /// Top reverse speed, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub reverse_speed: f32,
    /// How fast throttle speeds the hull up towards its top speed, in metres
    /// per second squared.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub acceleration: f32,
    /// How fast throttle against the hull's motion slows it down, in metres
    /// per second squared.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub braking: f32,
    /// How fast the hull coasts to a stop with no throttle, in metres per
    /// second squared.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub friction: f32,
    /// Hull turn rate, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub hull_turn_speed: f32,
    /// Fastest the turret turns towards the cursor, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub turret_turn_speed: f32,
    pub projectile_speed: f32,
    pub projectile_damage: f32,
    pub projectile_lifetime: f32,
    pub projectile_radius: f32,
    pub muzzle_offset: f32,
    pub max_ammo: u32,
    pub ammo: u32,
    pub fire_cooldown: f32,
    pub reload_duration: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub fire_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub reload_remaining: f32,
    /// Signed forward speed of the hull, in metres per second.
    #[serde(skip)]
    #[reflect_skip]
    pub speed: f32,
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
            acceleration: 18.0,
            braking: 28.0,
            friction: 14.0,
            hull_turn_speed: 2.8,
            turret_turn_speed: 12.0,
            projectile_speed: 28.0,
            projectile_damage: 35.0,
            projectile_lifetime: 2.0,
            projectile_radius: 0.18,
            muzzle_offset: 1.0,
            max_ammo: 6,
            ammo: 6,
            fire_cooldown: 0.35,
            reload_duration: 1.4,
            fire_cooldown_remaining: 0.0,
            reload_remaining: 0.0,
            speed: 0.0,
        }
    }
}

impl Component for ComponentTankController {}

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "951b5a6f-c3bb-46ed-b938-3909a2f439d1"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Projectile")]
#[serde(default)]
pub struct ComponentProjectile {
    pub direction: Vec3,
    pub speed: f32,
    pub damage: f32,
    pub lifetime_remaining: f32,
    pub radius: f32,
    /// The object that fired the round, credited with the kill when the
    /// round destroys its target.
    pub shooter: GameObjectRef,
}

impl Default for ComponentProjectile {
    fn default() -> Self {
        Self {
            direction: vec3(0.0, 0.0, 1.0),
            speed: 28.0,
            damage: 35.0,
            lifetime_remaining: 2.0,
            radius: 0.18,
            shooter: Default::default(),
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

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "8f0175f4-1a76-4c43-8932-6462f9005ab4"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Health")]
#[serde(default)]
pub struct ComponentHealth {
    pub max_health: f32,
    pub current_health: f32,
    pub destroy_on_death: bool,
    pub dead: bool,
}

impl Default for ComponentHealth {
    fn default() -> Self {
        Self {
            max_health: 100.0,
            current_health: 100.0,
            destroy_on_death: false,
            dead: false,
        }
    }
}

impl Component for ComponentHealth {}

impl ComponentHealth {
    fn apply_damage(&mut self, amount: f32) -> bool {
        if self.dead || amount <= 0.0 {
            return self.dead;
        }

        self.current_health = (self.current_health - amount).max(0.0);
        self.dead = self.current_health <= f32::EPSILON;
        self.dead
    }

    pub(crate) fn restore_full(&mut self) {
        self.max_health = self.max_health.max(0.0);
        self.current_health = self.max_health;
        self.dead = false;
    }
}

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

        if scene
            .read_component::<ComponentRespawnState, _, _>(game_object, |state| {
                state.is_gameplay_locked()
            })
            .unwrap_or(false)
        {
            // A dead or respawning tank comes back at rest.
            let _ = scene.write_component::<ComponentTankController, _>(game_object, |component| {
                component.speed = 0.0;
            });
            return;
        }

        let dt = resources.time().delta_time();
        let previous_tank_transform = scene.world_transform(game_object);
        let tank_transform = update_hull(scene, game_object, input, dt, &mut controller);
        // Follow with the camera first, so the cursor aims through the view
        // this frame draws.
        update_camera(
            scene,
            &controller,
            &previous_tank_transform,
            &tank_transform,
        );
        let aim_point =
            cursor_ground_intersection(scene, input, &controller, tank_transform.position.y);
        update_crosshair(scene, &controller, aim_point);
        update_turret(
            scene,
            dt,
            &controller,
            &previous_tank_transform,
            &tank_transform,
            aim_point,
        );
        update_shooting(scene, game_object, input, dt, &mut controller);
        let _ = scene.write_component::<ComponentTankController, _>(game_object, |component| {
            component.fire_cooldown_remaining = controller.fire_cooldown_remaining;
            component.reload_remaining = controller.reload_remaining;
            component.ammo = controller.ammo;
            component.speed = controller.speed;
        });
    }
}

fn update_hull(
    scene: &mut calyx_engine::scene::Scene,
    game_object: calyx_engine::scene::GameObject,
    input: &Input,
    dt: TimeType,
    controller: &mut ComponentTankController,
) -> Transform {
    let mut transform = scene.world_transform(game_object);
    let throttle = input.axis("move_forward");
    let steer = input.axis("move_right");

    let start_rotation = transform.rotation;
    let yaw = steer * controller.hull_turn_speed * dt;
    let turned = steer.abs() > f32::EPSILON && turn_hull(scene, &mut transform, yaw);
    let mut changed = turned;

    let start_speed = controller.speed;
    let (speed, distance) = advance_speed(start_speed, throttle, controller, dt);
    controller.speed = speed;
    if distance.abs() > f32::EPSILON {
        // Drive along the heading halfway through this frame's turn: the
        // chord of the arc the hull follows, whatever the frame time.
        let heading = if turned {
            start_rotation * UnitQuaternion::from_euler_angles(0.0, yaw * 0.5, 0.0)
        } else {
            start_rotation
        };
        let forward = flatten_xz(heading * Vec3::z());
        let (mut movement, contact) = sweep_hull(scene, &transform, forward * distance);
        // Against a wall the hull's top speed drops to the share of it that
        // runs along the wall: none head-on, nearly all of it when grazing.
        // The share depends only on the contact angle, and the frame is
        // driven again under that cap, so sliding does not depend on the
        // frame rate.
        if let Some(normal) = contact {
            let along = forward - normal * forward.dot(&normal);
            let share = along.magnitude().min(1.0);
            let capped = ComponentTankController {
                move_speed: controller.move_speed.max(0.0) * share,
                reverse_speed: controller.reverse_speed.max(0.0) * share,
                ..*controller
            };
            let (mut capped_speed, capped_distance) =
                advance_speed(start_speed, throttle, &capped, dt);
            if throttle.abs() <= f32::EPSILON {
                // Coasting never speeds up, so only the cap can apply.
                let limit = if capped_speed >= 0.0 {
                    capped.move_speed
                } else {
                    capped.reverse_speed
                };
                capped_speed = capped_speed.clamp(-limit, limit);
            }
            movement *= (capped_distance / distance).clamp(0.0, 1.0);
            controller.speed = capped_speed;
        }
        transform.translate(&movement);
        changed = true;
    }

    if changed {
        let local_scale = scene.transform(game_object).scale;
        scene.set_world_transform(game_object, transform.matrix());
        let _ = scene.write_component::<ComponentTransform, _>(game_object, |component| {
            component.transform.scale = local_scale;
        });
    }
    scene.world_transform(game_object)
}

/// Advances the hull's signed forward speed by `dt` seconds towards the speed
/// `throttle` asks for and returns the new speed and the distance covered.
///
/// Speed changes linearly: at `acceleration` while gaining speed, at
/// `braking` while throttle opposes the motion, and at `friction` with no
/// throttle. Each stretch is integrated exactly, so the result does not
/// depend on how a span of time is split into frames.
fn advance_speed(
    speed: f32,
    throttle: f32,
    controller: &ComponentTankController,
    dt: f32,
) -> (f32, f32) {
    let throttle = throttle.clamp(-1.0, 1.0);
    let coasting = throttle.abs() <= f32::EPSILON;
    let target = if coasting {
        0.0
    } else if throttle > 0.0 {
        throttle * controller.move_speed.max(0.0)
    } else {
        throttle * controller.reverse_speed.max(0.0)
    };

    let mut speed = speed;
    let mut remaining = dt.max(0.0);
    let mut distance = 0.0;
    // At most two stretches: braking to a stop, then accelerating away.
    for _ in 0..2 {
        if remaining <= 0.0 || speed == target {
            break;
        }
        let slowing = speed != 0.0 && (target - speed).signum() != speed.signum();
        let (rate, end) = if coasting {
            (controller.friction, target)
        } else if slowing {
            // Braking through zero hands over to acceleration there.
            let end = if target == 0.0 || target.signum() == speed.signum() {
                target
            } else {
                0.0
            };
            (controller.braking, end)
        } else {
            (controller.acceleration, target)
        };
        if rate <= 0.0 {
            break;
        }
        let time_to_end = (end - speed).abs() / rate;
        let (duration, next) = if time_to_end <= remaining {
            (time_to_end, end)
        } else {
            (remaining, speed + (end - speed).signum() * rate * remaining)
        };
        distance += (speed + next) * 0.5 * duration;
        speed = next;
        remaining -= duration;
    }
    distance += speed * remaining;
    (speed, distance)
}

/// Turns the hull by `yaw` radians unless the turned hull would overlap a
/// static obstacle it was not already overlapping. Returns whether it turned.
fn turn_hull(scene: &Scene, transform: &mut Transform, yaw: f32) -> bool {
    let rotation = transform.rotation * UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
    let obstacles = static_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    let overlaps = |rotation: UnitQuaternion<f32>| {
        scene
            .physics
            .intersect_shape(
                ColliderShape::Cuboid {
                    half_extents: TANK_COLLIDER_HALF_EXTENTS,
                },
                transform.position + TANK_COLLIDER_OFFSET,
                rotation,
                QueryFilter::default().predicate(&is_obstacle),
            )
            .is_some()
    };
    if overlaps(rotation) && !overlaps(transform.rotation) {
        return false;
    }
    transform.rotation = rotation;
    true
}

/// Entities whose colliders block the hull: colliders on a fixed rigid body
/// (on the object itself or an ancestor) or on no rigid body at all. Colliders
/// of dynamic or kinematic bodies, such as projectiles, never stop the hull.
fn static_obstacles(scene: &Scene) -> HashSet<Entity> {
    scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentCollider, _, _>(object, |_| ())
                .is_some()
        })
        .filter(|&object| {
            let body_type = scene
                .read_component::<ComponentRigidBody, _, _>(object, |body| body.ty)
                .or_else(|| {
                    scene
                        .ancestor_with::<ComponentRigidBody>(object)
                        .and_then(|ancestor| {
                            scene.read_component::<ComponentRigidBody, _, _>(ancestor, |body| {
                                body.ty
                            })
                        })
                });
            matches!(body_type, None | Some(RigidBodyType::Fixed))
        })
        .map(|object| object.entity)
        .collect()
}

/// The movement [`sweep_hull`] manages, for tests of the sweep alone.
#[cfg(test)]
fn resolve_hull_movement(scene: &Scene, transform: &Transform, movement: Vec3) -> Vec3 {
    sweep_hull(scene, transform, movement).0
}

/// Moves the hull by `movement`, stopping at static obstacles and sliding
/// along them. Returns the movement it managed and the flattened normal of
/// the obstacle that opposed `movement` most, when it hit one.
fn sweep_hull(scene: &Scene, transform: &Transform, movement: Vec3) -> (Vec3, Option<Vec3>) {
    let physics = &scene.physics;
    let obstacles = static_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    let mut resolved = Vec3::zeros();
    let mut remaining = movement;
    let mut contact: Option<Vec3> = None;
    let opposition = |normal: &Vec3| normal.dot(&movement).abs();

    for _ in 0..2 {
        let distance = remaining.magnitude();
        if distance <= f32::EPSILON {
            break;
        }
        let Some(hit) = physics.cast_shape(
            ColliderShape::Cuboid {
                half_extents: TANK_COLLIDER_HALF_EXTENTS,
            },
            transform.position + resolved + TANK_COLLIDER_OFFSET,
            transform.rotation,
            remaining,
            1.0,
            QueryFilter::default().predicate(&is_obstacle),
        ) else {
            resolved += remaining;
            break;
        };

        let time_of_impact = hit.hit.time_of_impact.clamp(0.0, 1.0);
        let safe_time = (time_of_impact - COLLISION_SKIN / distance).max(0.0);
        resolved += remaining * safe_time;

        // Everything not travelled, including the skin held back, slides on.
        let untraveled = remaining * (1.0 - safe_time);
        // Rapier reports `normal1` on the hit collider in world space.
        let normal = hit.hit.normal1.into_inner();
        remaining = untraveled - normal * untraveled.dot(&normal);
        remaining.y = 0.0;
        let flat_normal = flatten_xz(normal);
        if contact.is_none_or(|contact| opposition(&flat_normal) > opposition(&contact)) {
            contact = Some(flat_normal);
        }
    }

    (resolved, contact)
}

fn update_turret(
    scene: &mut calyx_engine::scene::Scene,
    dt: TimeType,
    controller: &ComponentTankController,
    previous_tank_transform: &Transform,
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

    let max_turn = controller.turret_turn_speed.max(0.0) * dt.max(0.0);
    let _ = scene.write_component::<ComponentTransform, _>(turret_object, |transform| {
        // Turn in world space, at a fixed rate, from where the turret pointed
        // before this frame's hull turn, so its aim depends only on elapsed
        // time and not on the frame rate.
        let current_yaw =
            rotation_yaw(&(previous_tank_transform.rotation * transform.transform.rotation));
        let desired_yaw = aim_direction.x.atan2(aim_direction.z);
        let yaw = turn_towards(current_yaw, desired_yaw, max_turn);
        transform.transform.rotation =
            tank_transform.rotation.inverse() * UnitQuaternion::from_euler_angles(0.0, yaw, 0.0);
    });
}

/// Yaw in radians of `rotation`'s forward axis, from +Z towards +X.
fn rotation_yaw(rotation: &UnitQuaternion<f32>) -> f32 {
    let forward = rotation * Vec3::z();
    forward.x.atan2(forward.z)
}

/// Turns the angle `current` towards `target` the short way round, by at
/// most `max_step` radians.
fn turn_towards(current: f32, target: f32, max_step: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let difference = (target - current + PI).rem_euclid(TAU) - PI;
    current + difference.clamp(-max_step, max_step)
}

fn update_crosshair(
    scene: &mut calyx_engine::scene::Scene,
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
    scene: &mut calyx_engine::scene::Scene,
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
    scene: &mut calyx_engine::scene::Scene,
    tank: calyx_engine::scene::GameObject,
    input: &Input,
    dt: TimeType,
    controller: &mut ComponentTankController,
) {
    controller.fire_cooldown_remaining =
        advance_fire_cooldown(controller.fire_cooldown_remaining, dt);
    advance_reload(controller, dt);
    if is_reloading(controller) {
        return;
    }
    if !can_fire(
        input.action("shoot").pressed(),
        controller.fire_cooldown_remaining,
        controller.ammo,
    ) {
        return;
    }

    if spawn_projectile(scene, tank, controller) {
        controller.ammo = controller.ammo.saturating_sub(1);
        controller.fire_cooldown_remaining = controller.fire_cooldown;
        if controller.ammo == 0 {
            controller.reload_remaining = controller.reload_duration;
        }
    }
}

fn advance_fire_cooldown(remaining: f32, dt: f32) -> f32 {
    (remaining - dt).max(0.0)
}

fn advance_reload(controller: &mut ComponentTankController, dt: f32) {
    if controller.max_ammo == 0 {
        controller.ammo = 0;
        controller.reload_remaining = 0.0;
        return;
    }

    controller.ammo = controller.ammo.min(controller.max_ammo);
    if controller.ammo > 0 {
        controller.reload_remaining = 0.0;
        return;
    }

    if controller.reload_remaining <= f32::EPSILON {
        controller.ammo = controller.max_ammo;
        return;
    }

    controller.reload_remaining = (controller.reload_remaining - dt.max(0.0)).max(0.0);
    if controller.reload_remaining <= f32::EPSILON {
        controller.ammo = controller.max_ammo;
    }
}

fn is_reloading(controller: &ComponentTankController) -> bool {
    controller.ammo == 0
}

fn can_fire(shoot_pressed: bool, cooldown_remaining: f32, ammo: u32) -> bool {
    shoot_pressed && cooldown_remaining <= f32::EPSILON && ammo > 0
}

fn spawn_projectile(
    scene: &mut calyx_engine::scene::Scene,
    tank: calyx_engine::scene::GameObject,
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
        cast_shadows: mesh.cast_shadows,
    });
    spawn_round(
        scene,
        barrel_transform.position + direction * controller.muzzle_offset,
        barrel_transform.rotation,
        ComponentProjectile {
            direction,
            speed: controller.projectile_speed,
            damage: controller.projectile_damage,
            lifetime_remaining: controller.projectile_lifetime,
            radius: controller.projectile_radius,
            shooter: GameObjectRef::new(scene.uuid(tank)),
        },
        visual,
    );
    true
}

/// Spawns a "Projectile" object at `position` that flies as `projectile`
/// says, drawn with `visual` scaled to the round's diameter.
pub(crate) fn spawn_round(
    scene: &mut Scene,
    position: Vec3,
    rotation: UnitQuaternion<f32>,
    projectile: ComponentProjectile,
    visual: Option<ComponentMesh>,
) {
    let projectile_object = scene.create(
        Some(ComponentID {
            name: "Projectile".to_string(),
            ..Default::default()
        }),
        None,
    );
    let diameter = projectile.radius * 2.0;
    let projectile_transform =
        Transform::from_components(position, rotation, vec3(diameter, diameter, diameter));
    scene.set_world_transform(projectile_object, projectile_transform.matrix());
    scene.add_component(projectile_object, projectile);
    scene.add_component(
        projectile_object,
        ComponentRigidBody {
            gravity_scale: 0.0,
            can_sleep: false,
            ..Default::default()
        },
    );
    scene.add_component(
        projectile_object,
        ComponentCollider {
            shape: ColliderShape::Sphere {
                radius: projectile.radius,
            },
            ..Default::default()
        },
    );
    if let Some(visual) = visual {
        scene.add_component(projectile_object, visual);
    }
}

fn update_projectile(
    scene: &mut calyx_engine::scene::Scene,
    game_object: calyx_engine::scene::GameObject,
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

    // Static obstacles such as arena walls and cover absorb the round, so a
    // target only takes damage when it is reached before the first obstacle.
    let obstacle_time = first_obstacle_time(scene, start, end, projectile.radius);
    let reach = obstacle_time.map_or(end, |time| start + (end - start) * time);
    if let Some(target) = first_projectile_hit(scene, game_object, start, reach, projectile.radius)
    {
        let shooter = projectile.shooter.game_object(scene);
        apply_projectile_damage(scene, target, projectile.damage, shooter);
        scene.delete(game_object);
        return;
    }
    if obstacle_time.is_some() {
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

/// Fraction of the `start`..`end` segment a round of `radius` travels before
/// it touches a static obstacle, if it touches one.
fn first_obstacle_time(scene: &Scene, start: Vec3, end: Vec3, radius: f32) -> Option<f32> {
    let obstacles = static_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    scene
        .physics
        .cast_shape(
            ColliderShape::Sphere { radius },
            start,
            UnitQuaternion::identity(),
            end - start,
            1.0,
            QueryFilter::default().predicate(&is_obstacle),
        )
        .map(|hit| hit.hit.time_of_impact.clamp(0.0, 1.0))
}

/// Damages `target` by `damage` and, when the hit kills it, records the kill
/// for `shooter`.
fn apply_projectile_damage(
    scene: &mut calyx_engine::scene::Scene,
    target: calyx_engine::scene::GameObject,
    damage: f32,
    shooter: Option<calyx_engine::scene::GameObject>,
) {
    if scene
        .read_component::<ComponentRespawnState, _, _>(target, |state| state.is_gameplay_locked())
        .unwrap_or(false)
    {
        return;
    }

    let mut died = false;
    let mut killed = false;
    let mut should_destroy = false;
    let damaged_health = scene
        .write_component::<ComponentHealth, _>(target, |health| {
            let was_dead = health.dead;
            died = health.apply_damage(damage);
            killed = died && !was_dead;
            should_destroy = died && health.destroy_on_death;
        })
        .is_some();
    if killed {
        let event = KillEvent::new(scene, shooter, target);
        record_kill(scene, &event);
    }

    if !damaged_health {
        let _ = scene.write_component::<ComponentProjectileTarget, _>(target, |target| {
            target.hit_count = target.hit_count.saturating_add(1);
        });
    }

    let respawn_requested = died
        && scene
            .write_component::<ComponentRespawnState, _>(target, |state| {
                state.request_death();
            })
            .is_some();

    if should_destroy && !respawn_requested {
        scene.delete(target);
    }
}

fn first_projectile_hit(
    scene: &calyx_engine::scene::Scene,
    projectile_object: calyx_engine::scene::GameObject,
    start: Vec3,
    end: Vec3,
    projectile_radius: f32,
) -> Option<calyx_engine::scene::GameObject> {
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
    scene: &calyx_engine::scene::Scene,
    input: &Input,
    controller: &ComponentTankController,
    ground_y: f32,
) -> Option<Vec3> {
    let rect = input.surface_rect()?;
    let cursor = input.pointer_pos()?;
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
    let ray = camera.screen_ray(
        &camera_transform,
        vec2(cursor.x - rect.left(), cursor.y - rect.top()),
        vec2(rect.width(), rect.height()),
    )?;
    ground_intersection(&ray, ground_y)
}

fn ground_intersection(ray: &Ray, ground_y: f32) -> Option<Vec3> {
    if ray.direction.y.abs() <= f32::EPSILON {
        return None;
    }

    let t = (ground_y - ray.origin.y) / ray.direction.y;
    (t >= 0.0).then_some(ray.origin + ray.direction * t)
}

pub(crate) fn flatten_xz(vector: Vec3) -> Vec3 {
    let flattened = vec3(vector.x, 0.0, vector.z);
    if flattened.magnitude_squared() <= f32::EPSILON {
        Vec3::zeros()
    } else {
        flattened.normalize()
    }
}

pub(crate) fn yaw_rotation(direction: &Vec3) -> UnitQuaternion<f32> {
    UnitQuaternion::from_euler_angles(0.0, direction.x.atan2(direction.z), 0.0)
}

#[cfg(test)]
mod tests {
    use super::{
        advance_fire_cooldown, advance_reload, advance_speed, apply_projectile_damage, can_fire,
        flatten_xz, follow_camera_xz, ground_intersection, is_reloading, resolve_hull_movement,
        segment_intersects_sphere, spawn_projectile, turn_hull, turn_towards, update_projectile,
        yaw_rotation, ComponentHealth, ComponentProjectile, ComponentProjectileTarget,
        ComponentTankController,
    };
    use crate::combat::{ComponentCombatStats, ComponentKillFeed};
    use crate::spawn::{update_respawn_state, ComponentRespawnState, ComponentSpawnPoint};
    use calyx_engine::component::{
        ColliderShape, ComponentCollider, ComponentID, ComponentRigidBody, ComponentTransform,
    };
    use calyx_engine::math::Transform;
    use calyx_engine::render::Camera;
    use calyx_engine::scene::GameObjectRef;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec2, vec3};
    use rapier3d::dynamics::RigidBodyType;
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

        let viewport = vec2(1280.0, 720.0);
        let camera = Camera::new(viewport.x / viewport.y, 70.0f32.to_radians(), 0.1, 1000.0);
        let ray = camera
            .screen_ray(&camera_transform, viewport * 0.5, viewport)
            .expect("center ray should exist");
        let hit = ground_intersection(&ray, tank_transform.position.y)
            .expect("center ray should intersect the tank ground plane");

        assert!((hit.x - tank_transform.position.x).abs() < 1e-3);
        assert!((hit.y - tank_transform.position.y).abs() < 1e-3);
    }

    #[test]
    fn sandbox_scene_camera_points_down_at_arena() {
        let scene: Value =
            serde_json::from_str(include_str!("../assets/game/scenes/arena.cxscene"))
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
        let test_assets = calyx_engine::test_support::TestAssets::sandbox();
        let assets = test_assets.asset_context();
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<calyx_engine::scene::Scene>(
                &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
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

        let mut game = calyx_engine::context::GameContext::new(assets);
        game.scenes.load_scene(scene_ref.readonly());
        assert!(
            game.scenes.current_scene().objects().count() > 0,
            "scene manager should install deserialized scene objects"
        );
    }

    #[test]
    fn editor_simulation_start_preserves_sandbox_camera_transform() {
        let test_assets = calyx_engine::test_support::TestAssets::sandbox();
        let assets = test_assets.asset_context();
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<calyx_engine::scene::Scene>(
                &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
            )
            .expect("sandbox scene should load");

        let mut game = calyx_engine::context::GameContext::new(assets);
        game.scenes.load_scene(scene_ref.readonly());
        let authoring_camera_transform = {
            let scene = game.scenes.current_scene();
            let (camera, _) = scene
                .main_camera()
                .expect("sandbox scene should have a main camera");
            scene.world_transform(camera)
        };
        let authoring_tank_scale = {
            let scene = game.scenes.current_scene();
            let tank = scene
                .objects()
                .find(|go| scene.name(*go) == "Tank")
                .expect("sandbox scene should have a Tank");
            scene.transform(tank).scale
        };

        game.scenes.start_simulation();

        let simulation_camera_transform = {
            let scene = game.scenes.simulation_scene();
            let (camera, _) = scene
                .main_camera()
                .expect("simulation scene should have a main camera");
            scene.world_transform(camera)
        };

        assert_eq!(
            simulation_camera_transform.position,
            authoring_camera_transform.position
        );
        assert_eq!(
            simulation_camera_transform.rotation,
            authoring_camera_transform.rotation
        );
        let simulation_tank_scale = {
            let scene = game.scenes.simulation_scene();
            let tank = scene
                .objects()
                .find(|go| scene.name(*go) == "Tank")
                .expect("simulation scene should have a Tank");
            scene.transform(tank).scale
        };
        assert_eq!(simulation_tank_scale, authoring_tank_scale);
    }

    #[test]
    fn editor_simulation_first_update_preserves_camera_without_input() {
        let test_assets = calyx_engine::test_support::TestAssets::sandbox();
        let assets = test_assets.asset_context();
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<calyx_engine::scene::Scene>(
                &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
            )
            .expect("sandbox scene should load");

        let mut game = calyx_engine::context::GameContext::new(assets);
        game.scenes.load_scene(scene_ref.readonly());
        let authoring_object_count = game.scenes.current_scene().objects().count();
        game.scenes.start_simulation();
        assert_eq!(
            game.scenes.simulation_scene().objects().count(),
            authoring_object_count + 68
        );
        let initial_camera_transform = {
            let scene = game.scenes.simulation_scene();
            let (camera, _) = scene
                .main_camera()
                .expect("simulation scene should have a main camera");
            scene.world_transform(camera)
        };
        let initial_tank_scale = {
            let scene = game.scenes.simulation_scene();
            let tank = scene
                .objects()
                .find(|go| scene.name(*go) == "Tank")
                .expect("simulation scene should have a Tank");
            scene.transform(tank).scale
        };

        let ctx = egui::Context::default();
        let input =
            calyx_engine::input::Input::from_ctx(&ctx, None, game.resources.input_mut(), false);
        let registries = game.assets.lock_read().registries;
        game.scenes.update(&registries, &mut game.resources, &input);

        let updated_camera_transform = {
            let scene = game.scenes.simulation_scene();
            let (camera, _) = scene
                .main_camera()
                .expect("simulation scene should have a main camera");
            scene.world_transform(camera)
        };

        assert_eq!(
            updated_camera_transform.position,
            initial_camera_transform.position
        );

        let updated_tank_scale = {
            let scene = game.scenes.simulation_scene();
            let tank = scene
                .objects()
                .find(|go| scene.name(*go) == "Tank")
                .expect("simulation scene should have a Tank");
            scene.transform(tank).scale
        };

        assert_eq!(updated_tank_scale, initial_tank_scale);
    }

    #[test]
    fn sandbox_shooting_does_not_move_targets() {
        let test_assets = calyx_engine::test_support::TestAssets::sandbox();
        let assets = test_assets.asset_context();
        let scene_ref = assets
            .registries
            .assets
            .read()
            .reload_by_path::<calyx_engine::scene::Scene>(
                &test_assets.sandbox_root().join("game/scenes/arena.cxscene"),
            )
            .expect("sandbox scene should load");
        let scene = scene_ref.read().clone();
        let target_transforms = ["Target A", "Target B"].map(|name| {
            let target = scene
                .objects()
                .find(|go| scene.name(*go) == name)
                .expect("sandbox scene should have target");
            (name, scene.world_transform(target))
        });

        let mut runner = calyx_engine::test_support::HeadlessSceneRunner::from_scene(scene);
        runner.press_key(egui::Key::Space);
        runner.step_many(120);

        for (name, transform) in target_transforms {
            let scene = runner.scene();
            let target = scene
                .objects()
                .find(|go| scene.name(*go) == name)
                .expect("sandbox scene should still have target");
            let updated = scene.world_transform(target);
            assert_eq!(updated.position, transform.position);
            assert_eq!(updated.rotation, transform.rotation);
            assert_eq!(updated.scale, transform.scale);
        }
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
    fn hull_movement_stops_before_middle_cover() {
        let mut scene = calyx_engine::test_support::test_scene();
        add_fixed_cube(&mut scene, vec3(0.0, 1.0, 3.0), vec3(1.0, 1.0, 1.0));
        scene.prepare();

        let movement = resolve_hull_movement(
            &scene,
            &Transform::from_xyz(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 4.0),
        );

        assert!(movement.z > 0.9);
        assert!(movement.z < 1.0);
    }

    #[test]
    fn diagonal_hull_movement_slides_along_wall() {
        let mut scene = calyx_engine::test_support::test_scene();
        add_fixed_cube(&mut scene, vec3(2.0, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
        scene.prepare();

        let movement = resolve_hull_movement(
            &scene,
            &Transform::from_xyz(0.0, 0.0, 0.0),
            vec3(3.0, 0.0, 2.0),
        );

        assert!(movement.x > 0.7);
        assert!(movement.x < 0.75);
        assert!(movement.z > 1.9);
    }

    #[test]
    fn yawed_hull_slides_along_wall_using_world_normal() {
        let mut scene = calyx_engine::test_support::test_scene();
        add_fixed_cube(&mut scene, vec3(2.0, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
        scene.prepare();
        let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);
        transform.rotation =
            UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0);

        let movement = resolve_hull_movement(&scene, &transform, vec3(3.0, 0.0, 2.0));

        assert!(movement.x > 0.45, "movement: {movement:?}");
        assert!(movement.x < 0.5, "movement: {movement:?}");
        assert!(movement.z > 1.9, "movement: {movement:?}");
    }

    #[test]
    fn hull_movement_ignores_projectile_colliders() {
        let mut scene = calyx_engine::test_support::test_scene();
        let projectile = scene.create(None, None);
        scene.set_transform(projectile, &Transform::from_xyz(0.0, 0.75, 2.0).matrix());
        scene.add_component(
            projectile,
            ComponentRigidBody {
                gravity_scale: 0.0,
                ..Default::default()
            },
        );
        scene.add_component(
            projectile,
            ComponentCollider {
                shape: ColliderShape::Sphere { radius: 0.18 },
                ..Default::default()
            },
        );
        scene.prepare();

        let movement = resolve_hull_movement(
            &scene,
            &Transform::from_xyz(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 4.0),
        );

        assert!((movement.z - 4.0).abs() < 1e-4, "movement: {movement:?}");
    }

    #[test]
    fn hull_does_not_turn_its_corners_into_a_wall() {
        let mut scene = calyx_engine::test_support::test_scene();
        // The wall face sits at x = 0.8, just clear of the hull's 0.75 half
        // width; a quarter turn would swing the 1.0 half length into it.
        add_fixed_cube(&mut scene, vec3(1.3, 1.0, 0.0), vec3(0.5, 1.0, 10.0));
        scene.prepare();
        let mut transform = Transform::from_xyz(0.0, 0.0, 0.0);

        assert!(!turn_hull(
            &scene,
            &mut transform,
            std::f32::consts::FRAC_PI_2
        ));
        assert_eq!(transform.rotation, UnitQuaternion::identity());

        let mut open_transform = Transform::from_xyz(-5.0, 0.0, 0.0);
        assert!(turn_hull(
            &scene,
            &mut open_transform,
            std::f32::consts::FRAC_PI_2
        ));
    }

    #[test]
    fn throttle_accelerates_to_top_speed_and_coasting_stops() {
        let controller = ComponentTankController::default();
        let to_top = controller.move_speed / controller.acceleration;

        let (speed, distance) = advance_speed(0.0, 1.0, &controller, to_top * 0.5);
        assert!((speed - controller.move_speed * 0.5).abs() < 1e-4);
        assert!((distance - controller.move_speed * 0.5 * to_top * 0.25).abs() < 1e-4);

        let (speed, distance) = advance_speed(0.0, 1.0, &controller, to_top + 1.0);
        assert_eq!(speed, controller.move_speed);
        assert!((distance - (controller.move_speed * (to_top * 0.5 + 1.0))).abs() < 1e-3);

        let (speed, _) = advance_speed(0.0, -1.0, &controller, 10.0);
        assert_eq!(speed, -controller.reverse_speed);

        let to_rest = controller.move_speed / controller.friction;
        let (speed, distance) = advance_speed(controller.move_speed, 0.0, &controller, 10.0);
        assert_eq!(speed, 0.0);
        assert!((distance - controller.move_speed * to_rest * 0.5).abs() < 1e-3);
    }

    #[test]
    fn reverse_throttle_brakes_then_reverses() {
        let controller = ComponentTankController::default();
        let to_rest = controller.move_speed / controller.braking;

        let (speed, distance) = advance_speed(controller.move_speed, -1.0, &controller, to_rest);
        assert!(speed.abs() < 1e-4);
        assert!((distance - controller.move_speed * to_rest * 0.5).abs() < 1e-4);

        let (speed, _) = advance_speed(controller.move_speed, -1.0, &controller, to_rest + 0.1);
        assert!((speed + controller.acceleration * 0.1).abs() < 1e-3);
    }

    #[test]
    fn speed_does_not_depend_on_how_time_is_split() {
        let controller = ComponentTankController::default();
        let inputs = [(1.0, 0.8), (-1.0, 0.7), (0.0, 1.0), (-1.0, 0.4), (1.0, 0.6)];
        let drive = |frames_per_second: usize| {
            let dt = 1.0 / frames_per_second as f32;
            let mut speed = 0.0;
            let mut distance = 0.0;
            for (throttle, duration) in inputs {
                for _ in 0..(duration * frames_per_second as f32).round() as usize {
                    let (next, step) = advance_speed(speed, throttle, &controller, dt);
                    speed = next;
                    distance += step;
                }
            }
            (speed, distance)
        };

        let (slow_speed, slow_distance) = drive(30);
        let (fast_speed, fast_distance) = drive(240);
        assert!((slow_speed - fast_speed).abs() < 1e-3);
        assert!((slow_distance - fast_distance).abs() < 1e-3);
    }

    #[test]
    fn turret_turns_the_short_way_at_most_its_step() {
        use std::f32::consts::PI;
        assert!((turn_towards(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
        assert!((turn_towards(0.0, 0.1, 0.25) - 0.1).abs() < 1e-6);
        // From just short of +pi to just past -pi is a small turn through pi.
        let turned = turn_towards(PI - 0.1, -PI + 0.1, 0.05);
        assert!((turned - (PI - 0.05)).abs() < 1e-5);
    }

    fn add_fixed_cube(
        scene: &mut calyx_engine::scene::Scene,
        position: nalgebra_glm::Vec3,
        half_extents: nalgebra_glm::Vec3,
    ) {
        let object = scene.create(None, None);
        scene.set_transform(
            object,
            &Transform::from_components(position, Default::default(), vec3(1.0, 1.0, 1.0)).matrix(),
        );
        scene.add_component(
            object,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                ..Default::default()
            },
        );
        scene.add_component(
            object,
            ComponentCollider {
                shape: ColliderShape::Cuboid { half_extents },
                ..Default::default()
            },
        );
    }

    #[test]
    fn fire_cooldown_counts_down_to_zero() {
        assert!((advance_fire_cooldown(0.35, 0.1) - 0.25).abs() < 1e-6);
        assert_eq!(advance_fire_cooldown(0.1, 0.35), 0.0);
    }

    #[test]
    fn fire_gate_requires_input_and_expired_cooldown() {
        assert!(can_fire(true, 0.0, 1));
        assert!(!can_fire(false, 0.0, 1));
        assert!(!can_fire(true, 0.1, 1));
        assert!(!can_fire(true, 0.0, 0));
    }

    #[test]
    fn reload_refills_empty_ammo_after_duration() {
        let mut controller = ComponentTankController {
            max_ammo: 3,
            ammo: 0,
            reload_duration: 1.0,
            reload_remaining: 1.0,
            ..Default::default()
        };

        advance_reload(&mut controller, 0.4);
        assert_eq!(controller.ammo, 0);
        assert!(is_reloading(&controller));
        assert!((controller.reload_remaining - 0.6).abs() < 1e-6);

        advance_reload(&mut controller, 0.6);
        assert_eq!(controller.ammo, 3);
        assert!(!is_reloading(&controller));
        assert_eq!(controller.reload_remaining, 0.0);
    }

    #[test]
    fn reload_is_disabled_when_max_ammo_is_zero() {
        let mut controller = ComponentTankController {
            max_ammo: 0,
            ammo: 0,
            reload_remaining: 1.0,
            ..Default::default()
        };

        advance_reload(&mut controller, 1.0);

        assert_eq!(controller.ammo, 0);
        assert_eq!(controller.reload_remaining, 0.0);
    }

    #[test]
    fn reload_clamps_stale_ammo_to_max_ammo() {
        let mut controller = ComponentTankController {
            max_ammo: 3,
            ammo: 6,
            reload_remaining: 1.0,
            ..Default::default()
        };

        advance_reload(&mut controller, 0.1);

        assert_eq!(controller.ammo, 3);
        assert_eq!(controller.reload_remaining, 0.0);
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

    #[test]
    fn spawned_projectile_has_velocity_and_physics_components() {
        let mut scene = calyx_engine::test_support::test_scene();
        let barrel = scene.create(
            Some(ComponentID {
                name: "Barrel".to_string(),
                ..Default::default()
            }),
            None,
        );
        scene.set_world_transform(
            barrel,
            Transform::from_components(
                vec3(0.0, 0.0, 0.0),
                UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        let controller = ComponentTankController {
            barrel: GameObjectRef::new(scene.uuid(barrel)),
            projectile_radius: 0.25,
            projectile_speed: 12.0,
            projectile_damage: 17.0,
            projectile_lifetime: 1.5,
            ..Default::default()
        };

        assert!(spawn_projectile(&mut scene, barrel, &controller));

        let projectile = scene
            .objects()
            .find(|object| scene.name(*object) == "Projectile")
            .expect("spawn should create a projectile object");
        let projectile_component = scene
            .read_component::<ComponentProjectile, _, _>(projectile, |component| *component)
            .expect("projectile should have velocity/lifetime data");
        assert_eq!(projectile_component.speed, 12.0);
        assert_eq!(projectile_component.damage, 17.0);
        assert_eq!(projectile_component.lifetime_remaining, 1.5);
        assert_eq!(projectile_component.radius, 0.25);
        assert!((projectile_component.direction.x - 1.0).abs() < 1e-5);
        assert!(projectile_component.direction.z.abs() < 1e-5);
        assert!(
            scene
                .world_transform(projectile)
                .rotation
                .angle_to(&scene.world_transform(barrel).rotation)
                < 1e-5
        );

        let rigid_body = scene
            .read_component::<ComponentRigidBody, _, _>(projectile, |component| {
                (component.gravity_scale, component.can_sleep)
            })
            .expect("projectile should have a rigid body");
        assert_eq!(rigid_body.0, 0.0);
        assert!(!rigid_body.1);

        let collider = scene
            .read_component::<ComponentCollider, _, _>(projectile, |component| component.shape)
            .expect("projectile should have a collider");
        let ColliderShape::Sphere { radius } = collider else {
            panic!("projectile collider should be a sphere");
        };
        assert_eq!(radius, controller.projectile_radius);

        let expected_rotation = scene.world_transform(barrel).rotation;
        scene.prepare();
        let mut runner = calyx_engine::test_support::HeadlessSceneRunner::from_scene(scene);
        for step in 1..=10 {
            runner.step();
            let projectile = runner
                .scene()
                .objects()
                .find(|object| runner.scene().name(*object) == "Projectile")
                .expect("projectile should survive the first updates");
            let actual_rotation = runner.scene().world_transform(projectile).rotation;
            let angle = actual_rotation.angle_to(&expected_rotation);
            assert!(
                angle < 1e-5,
                "projectile rotation changed by {angle} radians on step {step}: {actual_rotation:?}"
            );
        }
    }

    #[test]
    fn projectile_hit_applies_damage_to_health() {
        let mut scene = calyx_engine::test_support::test_scene();
        let projectile = scene.create(
            Some(ComponentID {
                name: "Projectile".to_string(),
                ..Default::default()
            }),
            None,
        );
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage: 25.0,
                lifetime_remaining: 1.0,
                radius: 0.1,
                ..Default::default()
            },
        );

        let target = scene.create(
            Some(ComponentID {
                name: "Target".to_string(),
                ..Default::default()
            }),
            None,
        );
        scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
        scene.add_component(target, ComponentProjectileTarget::default());
        scene.add_component(
            target,
            ComponentHealth {
                current_health: 60.0,
                max_health: 60.0,
                ..Default::default()
            },
        );

        update_projectile(&mut scene, projectile, 0.5);

        let health = scene
            .read_component::<ComponentHealth, _, _>(target, |health| *health)
            .expect("target should keep health after nonlethal hit");
        assert_eq!(health.current_health, 35.0);
        assert!(!health.dead);
        let hit_count = scene
            .read_component::<ComponentProjectileTarget, _, _>(target, |target| target.hit_count)
            .unwrap();
        assert_eq!(hit_count, 0, "health damage replaces legacy hit counting");
    }

    #[test]
    fn arena_wall_stops_projectile_before_target() {
        let mut scene = calyx_engine::test_support::test_scene();
        add_fixed_cube(&mut scene, vec3(0.0, 0.0, 2.5), vec3(5.0, 1.0, 0.25));
        let projectile = scene.create(None, None);
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage: 25.0,
                lifetime_remaining: 1.0,
                radius: 0.1,
                ..Default::default()
            },
        );
        let target = scene.create(None, None);
        scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
        scene.add_component(target, ComponentProjectileTarget::default());
        scene.add_component(
            target,
            ComponentHealth {
                current_health: 60.0,
                max_health: 60.0,
                ..Default::default()
            },
        );
        scene.prepare();

        update_projectile(&mut scene, projectile, 0.5);
        scene.flush_deletes();

        let health = scene
            .read_component::<ComponentHealth, _, _>(target, |health| health.current_health)
            .expect("target should keep its health");
        assert_eq!(health, 60.0, "the wall should absorb the round");
        assert!(
            !scene.objects().any(|object| object == projectile),
            "the round should be destroyed on the wall"
        );
    }

    #[test]
    fn projectile_is_destroyed_when_it_reaches_a_wall() {
        let mut scene = calyx_engine::test_support::test_scene();
        add_fixed_cube(&mut scene, vec3(0.0, 0.0, 2.5), vec3(5.0, 1.0, 0.25));
        let projectile = scene.create(None, None);
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage: 25.0,
                lifetime_remaining: 1.0,
                radius: 0.1,
                ..Default::default()
            },
        );
        scene.prepare();

        update_projectile(&mut scene, projectile, 0.1);
        scene.flush_deletes();
        assert!(
            scene.objects().any(|object| object == projectile),
            "the round has not reached the wall yet"
        );

        update_projectile(&mut scene, projectile, 0.5);
        scene.flush_deletes();
        assert!(!scene.objects().any(|object| object == projectile));
    }

    #[test]
    fn projectile_expires_after_its_lifetime_in_open_space() {
        let mut scene = calyx_engine::test_support::test_scene();
        let projectile = scene.create(None, None);
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage: 25.0,
                lifetime_remaining: 0.5,
                radius: 0.1,
                ..Default::default()
            },
        );
        scene.prepare();

        update_projectile(&mut scene, projectile, 0.2);
        scene.flush_deletes();
        let remaining = scene
            .read_component::<ComponentProjectile, _, _>(projectile, |component| {
                component.lifetime_remaining
            })
            .expect("the round outlives its first update");
        assert!((remaining - 0.3).abs() < 1e-5, "remaining {remaining}");
        let position = scene.world_transform(projectile).position;
        assert!((position.z - 2.0).abs() < 1e-5, "position {position:?}");

        update_projectile(&mut scene, projectile, 0.2);
        scene.flush_deletes();
        assert!(scene.objects().any(|object| object == projectile));

        // The last update covers only the 0.1 s left, then removes the round.
        update_projectile(&mut scene, projectile, 0.2);
        scene.flush_deletes();
        assert!(!scene.objects().any(|object| object == projectile));
    }

    #[test]
    fn lethal_projectile_hit_records_the_kill_for_its_shooter() {
        let mut scene = calyx_engine::test_support::test_scene();
        let tank = scene.create(
            Some(ComponentID {
                name: "Tank".to_string(),
                ..Default::default()
            }),
            None,
        );
        scene.add_component(tank, ComponentCombatStats::default());
        scene.add_component(tank, ComponentKillFeed::default());
        let target = scene.create(
            Some(ComponentID {
                name: "Target A".to_string(),
                ..Default::default()
            }),
            None,
        );
        scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
        scene.add_component(target, ComponentProjectileTarget::default());
        scene.add_component(
            target,
            ComponentHealth {
                current_health: 40.0,
                max_health: 40.0,
                destroy_on_death: true,
                ..Default::default()
            },
        );
        let fire = |scene: &mut calyx_engine::scene::Scene, damage: f32| {
            let projectile = scene.create(None, None);
            scene.add_component(
                projectile,
                ComponentProjectile {
                    direction: vec3(0.0, 0.0, 1.0),
                    speed: 10.0,
                    damage,
                    lifetime_remaining: 1.0,
                    radius: 0.1,
                    shooter: GameObjectRef::new(scene.uuid(tank)),
                },
            );
            update_projectile(scene, projectile, 0.5);
        };
        let kills = |scene: &calyx_engine::scene::Scene| {
            scene
                .read_component::<ComponentCombatStats, _, _>(tank, |stats| stats.kills)
                .unwrap()
        };

        fire(&mut scene, 25.0);
        assert_eq!(kills(&scene), 0, "a hit that does not kill is no kill");

        fire(&mut scene, 25.0);
        assert_eq!(kills(&scene), 1);
        let feed: Vec<(Option<String>, String)> = scene
            .read_component::<ComponentKillFeed, _, _>(tank, |feed| {
                feed.entries()
                    .map(|entry| (entry.killer.clone(), entry.victim.clone()))
                    .collect()
            })
            .unwrap();
        assert_eq!(
            feed,
            vec![(Some("Tank".to_string()), "Target A".to_string())]
        );
    }

    #[test]
    fn lethal_projectile_hit_marks_health_dead_and_can_destroy_target() {
        let mut scene = calyx_engine::test_support::test_scene();
        let projectile = scene.create(None, None);
        scene.add_component(
            projectile,
            ComponentProjectile {
                direction: vec3(0.0, 0.0, 1.0),
                speed: 10.0,
                damage: 100.0,
                lifetime_remaining: 1.0,
                radius: 0.1,
                ..Default::default()
            },
        );

        let target = scene.create(None, None);
        scene.set_world_transform(target, Transform::from_xyz(0.0, 0.0, 5.0).matrix());
        scene.add_component(target, ComponentProjectileTarget::default());
        scene.add_component(
            target,
            ComponentHealth {
                current_health: 40.0,
                max_health: 40.0,
                destroy_on_death: true,
                ..Default::default()
            },
        );

        update_projectile(&mut scene, projectile, 0.5);

        scene.flush_deletes();
        assert!(
            !scene.objects().any(|object| object == target),
            "destroy_on_death targets should be removed after lethal damage"
        );
    }

    #[test]
    fn lethal_damage_respawns_player_and_restores_health() {
        let mut scene = calyx_engine::test_support::test_scene();
        let spawn = scene.create(None, None);
        let player = scene.create(None, None);
        scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
        scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
        scene.add_component(spawn, ComponentSpawnPoint::default());
        scene.add_component(
            player,
            ComponentHealth {
                max_health: 40.0,
                current_health: 40.0,
                ..Default::default()
            },
        );
        scene.add_component(
            player,
            ComponentRespawnState {
                respawn_delay: 1.0,
                ..Default::default()
            },
        );

        apply_projectile_damage(&mut scene, player, 40.0, None);

        assert!(scene
            .read_component::<ComponentHealth, _, _>(player, |health| health.dead)
            .expect("player should keep health while awaiting respawn"));
        assert!(scene
            .read_component::<ComponentRespawnState, _, _>(player, |state| state.death_requested)
            .expect("lethal damage should request a respawn"));

        assert!(!update_respawn_state(&mut scene, player, 0.0));
        assert!(update_respawn_state(&mut scene, player, 1.0));

        let health = scene
            .read_component::<ComponentHealth, _, _>(player, |health| *health)
            .expect("respawned player should keep health");
        assert_eq!(health.current_health, 40.0);
        assert!(!health.dead);
        assert_eq!(scene.world_transform(player).position, vec3(8.0, 0.0, 4.0));
    }

    #[test]
    fn invulnerable_player_ignores_projectile_damage() {
        let mut scene = calyx_engine::test_support::test_scene();
        let player = scene.create(None, None);
        scene.add_component(player, ComponentHealth::default());
        scene.add_component(
            player,
            ComponentRespawnState {
                invulnerability_remaining: 1.0,
                ..Default::default()
            },
        );

        apply_projectile_damage(&mut scene, player, 25.0, None);

        let health = scene
            .read_component::<ComponentHealth, _, _>(player, |health| *health)
            .expect("player should keep health");
        assert_eq!(health.current_health, health.max_health);
        assert!(!health.dead);
    }

    #[test]
    fn waiting_to_respawn_player_ignores_projectile_damage() {
        let mut scene = calyx_engine::test_support::test_scene();
        let player = scene.create(None, None);
        scene.add_component(player, ComponentHealth::default());
        scene.add_component(
            player,
            ComponentRespawnState {
                alive: false,
                respawn_remaining: 1.0,
                ..Default::default()
            },
        );

        apply_projectile_damage(&mut scene, player, 25.0, None);

        let health = scene
            .read_component::<ComponentHealth, _, _>(player, |health| *health)
            .expect("player should keep health");
        let state = scene
            .read_component::<ComponentRespawnState, _, _>(player, |state| *state)
            .expect("player should keep its respawn timer");
        assert_eq!(health.current_health, health.max_health);
        assert!(!state.death_requested);
        assert_eq!(state.respawn_remaining, 1.0);
    }
}
