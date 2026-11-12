//! PROTOTYPE (poise_poc, #610): poise, physics knockdowns, a boot and a
//! grappling hook, played in `assets/game/scenes/poise_poc.cxscene` to judge
//! whether knocking tanks around is fun. Nothing here is production code:
//! remove it, or rebuild it properly, once play-testing has decided.
//!
//! Everything is gated on the prototype's own components, so the arena and
//! the main game flow never meet it:
//!
//! - [`ComponentPoise`] replaces health on a tank: rounds drain it and shove
//!   the tank, it regenerates after a delay, and at zero the tank's rigid
//!   body turns dynamic, takes the blow and tumbles. After the recovery time
//!   it turns kinematic again, rights itself onto its tracks and is immune
//!   for a while. A tumbling tank that hits an upright one drains its poise.
//! - [`ComponentDummyDriver`] drives a tank without input: a loop, or not at
//!   all.
//! - [`ComponentTankAbilities`](abilities::ComponentTankAbilities) gives the
//!   player the boot and the hook.
//! - [`ComponentPoiseHud`](hud::ComponentPoiseHud) draws poise, the
//!   abilities and the hook line.
//!
//! The tank controller asks [`tank_control`] who drives, and a round asks
//! [`absorb_round`] whether poise takes it; both answer as before for a tank
//! without these components.

pub mod abilities;
pub mod hud;
pub mod logic;

use crate::tank::{flatten_xz, hull_obstacles, level_rotation, rotation_yaw, sweep_hull};
use calyx_engine::component::{
    ColliderShape, Component, ComponentCollider, ComponentEventContext, ComponentRef,
    ComponentRigidBody, ComponentTransform, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use logic::{HitOutcome, PoiseState, PoiseTuning, Stance, Transition};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use rapier3d::prelude::{Collider, ColliderHandle, RigidBodyHandle, RigidBodyType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Below this height, or this far out, a tumbling tank has left the map and
/// goes back to where it started.
const LOST_BELOW: f32 = -10.0;
const LOST_BEYOND: f32 = 150.0;
/// The height of a poise tank's hull centre, where rounds hit it.
const HIT_CENTER_HEIGHT: f32 = 1.2;

/// PROTOTYPE (#610). A tank's poise, which replaces its health in the
/// prototype scene, and every tuning value of hits and knockdowns.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "006104e0-1cb3-4815-9faa-ff88b760c0e8"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Poise (prototype)")]
#[serde(default)]
pub struct ComponentPoise {
    /// Full poise.
    #[reflect_attr(min = 1.0, speed = 1.0)]
    pub max_poise: f32,
    /// Seconds without a hit before poise regenerates.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub regen_delay: f32,
    /// Poise regained per second once regenerating.
    #[reflect_attr(min = 0.0, speed = 1.0)]
    pub regen_rate: f32,
    /// Poise a round takes, as a share of the shooter's projectile damage.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub shot_damage_scale: f32,
    /// Speed a round that does not break poise shoves the tank at, in metres
    /// per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub shot_knockback: f32,
    /// How fast a shove dies away, in metres per second squared.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub knockback_decay: f32,
    /// Speed a breaking round launches the tank at along its flight, in
    /// metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub knockdown_speed: f32,
    /// Upward speed of a breaking round's launch, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub knockdown_lift: f32,
    /// Spin a break gives the tank, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub knockdown_spin: f32,
    /// Speed a round adds to a tank already tumbling, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub juggle_speed: f32,
    /// Gravity multiplier while tumbling; above 1 lands sooner and heavier.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub tumble_gravity_scale: f32,
    /// Linear damping while tumbling.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub tumble_linear_damping: f32,
    /// Angular damping while tumbling.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub tumble_angular_damping: f32,
    /// Seconds on physics before the tank rights itself.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub recovery_time: f32,
    /// Seconds the tank takes to right itself.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub righting_time: f32,
    /// How high the tank hops while righting itself, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub righting_hop: f32,
    /// Seconds after righting in which poise cannot break.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub immunity_time: f32,
    /// A tumbling tank faster than this, in metres per second, drains the
    /// poise of an upright tank it hits.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub impact_min_speed: f32,
    /// Poise a tumbling tank takes from a tank it hits, per metre per second
    /// of its speed.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub impact_poise_per_speed: f32,
    /// How close, in metres, a tumbling tank's centre must come to another's
    /// to hit it.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub impact_radius: f32,

    #[serde(skip)]
    #[reflect_skip]
    pub(crate) state: PoiseState,
    /// The shove a hit gave the upright tank, in metres per second.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) knockback: Vec3,
    /// Seconds since the last break, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) since_break: f32,
    /// The height the tank stood at when it went down, which the camera
    /// keeps while it tumbles.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) rest_height: f32,
    /// The pose righting started from and the upright pose it ends at.
    #[serde(skip)]
    #[reflect_skip]
    righting: Option<(Transform, Transform)>,
    /// Tanks this knockdown has already hit.
    #[serde(skip)]
    #[reflect_skip]
    impacted: Vec<GameObjectRef>,
    /// Where the tank started, for when it is thrown off the map.
    #[serde(skip)]
    #[reflect_skip]
    home: Option<Transform>,
}

impl Default for ComponentPoise {
    fn default() -> Self {
        Self {
            max_poise: 100.0,
            regen_delay: 1.6,
            regen_rate: 45.0,
            shot_damage_scale: 1.0,
            shot_knockback: 3.5,
            knockback_decay: 22.0,
            knockdown_speed: 7.0,
            knockdown_lift: 6.0,
            knockdown_spin: 6.0,
            juggle_speed: 3.0,
            tumble_gravity_scale: 1.6,
            tumble_linear_damping: 0.2,
            tumble_angular_damping: 0.5,
            recovery_time: 2.2,
            righting_time: 0.45,
            righting_hop: 0.9,
            immunity_time: 1.5,
            impact_min_speed: 4.0,
            impact_poise_per_speed: 6.0,
            impact_radius: 2.6,
            state: PoiseState::new(100.0),
            knockback: Vec3::zeros(),
            since_break: f32::INFINITY,
            rest_height: 0.0,
            righting: None,
            impacted: Vec::new(),
            home: None,
        }
    }
}

impl Component for ComponentPoise {}

impl ComponentPoise {
    pub(crate) fn tuning(&self) -> PoiseTuning {
        PoiseTuning {
            max_poise: self.max_poise,
            regen_delay: self.regen_delay,
            regen_rate: self.regen_rate,
            recovery_time: self.recovery_time,
            righting_time: self.righting_time,
            immunity_time: self.immunity_time,
        }
    }

    pub(crate) fn state(&self) -> &PoiseState {
        &self.state
    }

    /// How far through its knockdown the tank is, 0 to 1, counting the
    /// recovery and the righting.
    pub(crate) fn down_progress(&self) -> f32 {
        let total = (self.recovery_time + self.righting_time).max(f32::EPSILON);
        match self.state.stance {
            Stance::KnockedDown { elapsed } => (elapsed / total).clamp(0.0, 1.0),
            Stance::Righting { elapsed } => {
                ((self.recovery_time + elapsed) / total).clamp(0.0, 1.0)
            }
            Stance::Upright | Stance::Hooked => 0.0,
        }
    }
}

/// PROTOTYPE (#610). Drives a tank without input, so it can be shot, booted
/// and hooked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "32e77f14-ca1a-4a34-a01f-c496bc48406d"]
pub enum DummyMode {
    /// Drives round in a loop.
    #[default]
    DriveLoop,
    /// Stays where it is.
    StayPut,
}

/// PROTOTYPE (#610). A tank with this takes no player input: it drives
/// itself as `mode` says.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "3f683c07-7266-4d61-9f27-e701d8018c05"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Dummy Driver (prototype)")]
#[serde(default)]
pub struct ComponentDummyDriver {
    pub mode: DummyMode,
    /// Throttle while driving the loop, -1 to 1.
    #[reflect_attr(min = -1.0, max = 1.0, speed = 0.01)]
    pub throttle: f32,
    /// Steering while driving the loop, -1 to 1; with the throttle it sets
    /// how wide the loop is.
    #[reflect_attr(min = -1.0, max = 1.0, speed = 0.01)]
    pub steer: f32,
}

impl Default for ComponentDummyDriver {
    fn default() -> Self {
        Self {
            mode: DummyMode::DriveLoop,
            throttle: 0.6,
            steer: 0.35,
        }
    }
}

impl Component for ComponentDummyDriver {}

/// Who drives a tank this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TankControl {
    /// The player's input, as for every tank outside the prototype.
    Player,
    /// Nobody: the tank is knocked down, righting or hooked. The camera
    /// follows `camera_focus`.
    Locked { camera_focus: Vec3 },
    /// The dummy driver, with this throttle and steering.
    Dummy { throttle: f32, steer: f32 },
}

/// Who drives `tank` this frame: [`TankControl::Player`] for any tank
/// without the prototype's components.
pub fn tank_control(scene: &Scene, tank: GameObject) -> TankControl {
    let down = scene
        .read_component::<ComponentPoise, _, _>(tank, |poise| {
            (!poise.state.is_upright()).then_some(poise.rest_height)
        })
        .flatten();
    if let Some(rest_height) = down {
        let mut camera_focus = scene.world_transform(tank).position;
        camera_focus.y = rest_height;
        return TankControl::Locked { camera_focus };
    }
    match scene.read_component::<ComponentDummyDriver, _, _>(tank, |driver| *driver) {
        Some(driver) => match driver.mode {
            DummyMode::DriveLoop => TankControl::Dummy {
                throttle: driver.throttle.clamp(-1.0, 1.0),
                steer: driver.steer.clamp(-1.0, 1.0),
            },
            DummyMode::StayPut => TankControl::Dummy {
                throttle: 0.0,
                steer: 0.0,
            },
        },
        None => TankControl::Player,
    }
}

/// How far above a tank's origin rounds aim for: the middle of the hull of
/// a tank with poise, so rounds flying at gun height meet it. Zero for any
/// other target.
pub fn hit_center_offset(scene: &Scene, target: GameObject) -> Vec3 {
    if scene
        .read_component::<ComponentPoise, _, _>(target, |_| ())
        .is_some()
    {
        Vec3::y() * HIT_CENTER_HEIGHT
    } else {
        Vec3::zeros()
    }
}

/// A blow to a tank's poise: how much it takes, which way it pushes and how
/// hard, depending on whether it breaks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Blow {
    pub poise_damage: f32,
    /// Horizontal unit direction of the push.
    pub direction: Vec3,
    /// Shove speed when poise holds, in metres per second.
    pub shove: f32,
    /// Launch speed along `direction` when poise breaks.
    pub launch: f32,
    /// Upward launch speed when poise breaks.
    pub lift: f32,
    /// Spin when poise breaks, in radians per second.
    pub spin: f32,
    /// Speed added to a tank already tumbling.
    pub juggle: f32,
}

/// Lets `target`'s poise take a round of `damage` flying along `direction`.
/// Returns false, doing nothing, for a target without poise: its health
/// takes the round as before.
pub fn absorb_round(scene: &mut Scene, target: GameObject, damage: f32, direction: Vec3) -> bool {
    let Some(poise) = scene.read_component::<ComponentPoise, _, _>(target, Clone::clone) else {
        return false;
    };
    let mut direction = flatten_xz(direction);
    if direction == Vec3::zeros() {
        direction = Vec3::z();
    }
    strike(
        scene,
        target,
        Blow {
            poise_damage: damage * poise.shot_damage_scale,
            direction,
            shove: poise.shot_knockback,
            launch: poise.knockdown_speed,
            lift: poise.knockdown_lift,
            spin: poise.knockdown_spin,
            juggle: poise.juggle_speed,
        },
    );
    true
}

/// Deals `blow` to `target`'s poise: a shove when it holds, a knockdown when
/// it breaks, and a push to a tank already tumbling.
pub(crate) fn strike(scene: &mut Scene, target: GameObject, blow: Blow) -> Option<HitOutcome> {
    let outcome = scene.write_component::<ComponentPoise, _>(target, |poise| {
        let outcome = poise.state.hit(blow.poise_damage);
        if outcome == HitOutcome::Drained {
            poise.knockback += blow.direction * blow.shove;
        }
        outcome
    })?;
    match outcome {
        HitOutcome::Drained => {}
        HitOutcome::Broken => knock_down(
            scene,
            target,
            blow.direction * blow.launch + Vec3::y() * blow.lift,
            blow.spin,
        ),
        HitOutcome::AlreadyDown => {
            let tumbling = scene
                .read_component::<ComponentPoise, _, _>(target, |poise| {
                    matches!(poise.state.stance, Stance::KnockedDown { .. })
                })
                .unwrap_or(false);
            if tumbling {
                if let Some(body) = body_handle(scene, target) {
                    let body = &mut scene.physics.bodies[body];
                    let push = blow.direction * blow.juggle + Vec3::y() * blow.juggle * 0.5;
                    body.set_linvel(body.linvel() + push, true);
                }
            }
        }
    }
    Some(outcome)
}

/// Puts `tank` down: its rigid body turns dynamic and flies off at
/// `velocity`, tumbling at `spin` radians per second about the horizontal
/// axis square to it, so it rolls over the way it was hit.
pub(crate) fn knock_down(scene: &mut Scene, tank: GameObject, velocity: Vec3, spin: f32) {
    let position = scene.world_transform(tank).position;
    let Some(poise) = scene.write_component::<ComponentPoise, _>(tank, |poise| {
        if poise.state.is_upright() {
            poise.rest_height = position.y;
        }
        poise.state.knock_down();
        poise.knockback = Vec3::zeros();
        poise.since_break = 0.0;
        poise.righting = None;
        poise.impacted.clear();
        poise.clone()
    }) else {
        return;
    };
    set_body_type(scene, tank, RigidBodyType::Dynamic, &poise);
    let Some(body) = body_handle(scene, tank) else {
        return;
    };
    let flat = flatten_xz(velocity);
    let axis = if flat == Vec3::zeros() {
        Vec3::x()
    } else {
        Vec3::y().cross(&flat)
    };
    let body = &mut scene.physics.bodies[body];
    body.set_linvel(velocity, true);
    // Mostly a roll over, with a little yaw so no two tumbles look alike.
    body.set_angvel(axis * spin + Vec3::y() * spin * 0.35, true);
}

/// Switches `tank`'s rigid body between kinematic control and dynamic
/// physics, both its component and its Rapier body, so the switch takes
/// effect this frame.
fn set_body_type(scene: &mut Scene, tank: GameObject, ty: RigidBodyType, poise: &ComponentPoise) {
    let dynamic = ty == RigidBodyType::Dynamic;
    let gravity_scale = if dynamic {
        poise.tumble_gravity_scale
    } else {
        1.0
    };
    let _ = scene.write_component::<ComponentRigidBody, _>(tank, |body| {
        body.ty = ty;
        body.gravity_scale = gravity_scale;
    });
    let Some(handle) = body_handle(scene, tank) else {
        return;
    };
    let body = &mut scene.physics.bodies[handle];
    body.set_body_type(ty, true);
    body.set_gravity_scale(gravity_scale, true);
    body.enable_ccd(dynamic);
    if dynamic {
        body.set_linear_damping(poise.tumble_linear_damping);
        body.set_angular_damping(poise.tumble_angular_damping);
    } else {
        body.set_linvel(Vec3::zeros(), true);
        body.set_angvel(Vec3::zeros(), true);
    }
}

/// The Rapier body of `tank`: the parent of the colliders on it and its
/// descendants.
pub(crate) fn body_handle(scene: &Scene, tank: GameObject) -> Option<RigidBodyHandle> {
    let entities: HashSet<_> = scene
        .descendants(tank)
        .map(|object| object.entity)
        .collect();
    scene
        .physics
        .colliders
        .iter()
        .find_map(|(handle, collider)| {
            scene
                .physics
                .collider_entity(handle)
                .filter(|entity| entities.contains(entity))
                .and(collider.parent())
        })
}

/// The velocity of `tank`'s body: the physics' while it tumbles, zero while
/// kinematic.
pub(crate) fn body_velocity(scene: &Scene, tank: GameObject) -> Vec3 {
    body_handle(scene, tank)
        .and_then(|handle| scene.physics.bodies.get(handle))
        .filter(|body| body.is_dynamic())
        .map_or_else(Vec3::zeros, |body| *body.linvel())
}

/// Whether the straight line from `from` to `to` is clear of every static
/// obstacle a hull would stop at, such as walls and cover.
pub(crate) fn line_clear(scene: &Scene, from: Vec3, to: Vec3) -> bool {
    let obstacles = hull_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    let along = to - from;
    let distance = along.magnitude();
    distance <= f32::EPSILON
        || scene
            .physics
            .cast_ray(
                from,
                along,
                distance,
                true,
                crate::collision::HULL
                    .query_filter()
                    .predicate(&is_obstacle),
            )
            .is_none()
}

/// Whether a hull moving from `from` to `to` would hit a static obstacle.
pub(crate) fn hull_blocked(scene: &Scene, from: &Transform, to: Vec3) -> bool {
    let movement = to - from.position;
    if movement.magnitude_squared() <= f32::EPSILON {
        return false;
    }
    sweep_hull(scene, from, movement).1.is_some()
}

/// Places `object` at `position` and `rotation` in the world, keeping its
/// local scale.
pub(crate) fn place(
    scene: &mut Scene,
    object: GameObject,
    position: Vec3,
    rotation: UnitQuaternion<f32>,
) {
    let scale = scene.transform(object).scale;
    let world_scale = scene.world_transform(object).scale;
    scene.set_world_transform(
        object,
        Transform::from_components(position, rotation, world_scale).matrix(),
    );
    let _ = scene.write_component::<ComponentTransform, _>(object, |component| {
        component.transform.scale = scale;
    });
}

/// The height of the drive surface under `position`, if there is one within
/// a few metres.
fn ground_height(scene: &Scene, position: Vec3) -> Option<f32> {
    let surfaces = crate::terrain::drive_surfaces(scene);
    crate::terrain::surface_height_below(scene, &surfaces, position + Vec3::y() * 3.0, 8.0)
}

/// The upright pose a tank lying at `pose` rights itself to: on the ground
/// under it, facing the way its nose points, or its belly when the nose
/// points straight up or down.
fn upright_pose(scene: &Scene, pose: &Transform, rest_height: f32) -> (Vec3, UnitQuaternion<f32>) {
    let mut heading = flatten_xz(pose.rotation * Vec3::z());
    if heading == Vec3::zeros() {
        heading = flatten_xz(pose.rotation * -Vec3::y());
    }
    let yaw = if heading == Vec3::zeros() {
        rotation_yaw(&pose.rotation)
    } else {
        heading.x.atan2(heading.z)
    };
    let mut position = pose.position;
    position.y = ground_height(scene, position).unwrap_or(rest_height);
    (position, UnitQuaternion::from_euler_angles(0.0, yaw, 0.0))
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl ComponentUpdate for ComponentPoise {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        if crate::game_state::combat_paused(scene) {
            return;
        }
        let dt = resources.time().delta_time().max(0.0);
        let Some(mut poise) = this.read(scene, Clone::clone) else {
            return;
        };
        let pose = scene.world_transform(game_object);
        let home = *poise.home.get_or_insert(pose);
        poise.since_break += dt;
        let tuning = poise.tuning();
        let transition = poise.state.tick(dt, &tuning);

        // Thrown off the map: back to the start, upright.
        let lost = pose.position.y < LOST_BELOW
            || pose.position.x.abs() > LOST_BEYOND
            || pose.position.z.abs() > LOST_BEYOND;
        if lost && poise.state.stance != Stance::Hooked {
            set_body_type(
                scene,
                game_object,
                RigidBodyType::KinematicPositionBased,
                &poise,
            );
            place(scene, game_object, home.position, home.rotation);
            poise.state.recover(&tuning);
            poise.righting = None;
            let _ = this.write(scene, |component| *component = poise);
            return;
        }

        match poise.state.stance {
            Stance::Upright => {
                if transition == Some(Transition::Recovered) {
                    if let Some((_, upright)) = poise.righting.take() {
                        place(scene, game_object, upright.position, upright.rotation);
                    }
                }
                apply_knockback(scene, game_object, &mut poise, dt);
            }
            Stance::KnockedDown { .. } => {
                poise.impacted = impact_tanks(scene, game_object, &poise);
            }
            Stance::Righting { .. } => {
                if transition == Some(Transition::StartRighting) || poise.righting.is_none() {
                    set_body_type(
                        scene,
                        game_object,
                        RigidBodyType::KinematicPositionBased,
                        &poise,
                    );
                    let (position, rotation) = upright_pose(scene, &pose, poise.rest_height);
                    let upright = Transform::from_components(position, rotation, pose.scale);
                    poise.righting = Some((pose, upright));
                }
                if let Some((from, to)) = poise.righting {
                    let t = smoothstep(poise.state.righting_progress(&tuning));
                    let hop = (t * std::f32::consts::PI).sin() * poise.righting_hop;
                    let position = from.position.lerp(&to.position, t) + Vec3::y() * hop;
                    let rotation = from.rotation.slerp(&to.rotation, t);
                    place(scene, game_object, position, rotation);
                }
            }
            Stance::Hooked => {}
        }
        let _ = this.write(scene, |component| *component = poise);
    }
}

/// Moves the upright `tank` by the shove it was given, stopping at walls,
/// and lets the shove die away.
fn apply_knockback(scene: &mut Scene, tank: GameObject, poise: &mut ComponentPoise, dt: f32) {
    let speed = poise.knockback.magnitude();
    if speed <= 1e-3 {
        poise.knockback = Vec3::zeros();
        return;
    }
    let mut transform = scene.world_transform(tank);
    transform.rotation = level_rotation(&transform.rotation);
    let (movement, contact) = sweep_hull(scene, &transform, poise.knockback * dt);
    if movement != Vec3::zeros() {
        let pose = scene.world_transform(tank);
        place(scene, tank, pose.position + movement, pose.rotation);
    }
    if let Some(normal) = contact {
        // A wall takes the part of the shove pushing into it.
        let into = poise.knockback.dot(&normal);
        if into < 0.0 {
            poise.knockback -= normal * into;
        }
    }
    let slowed = (speed - poise.knockback_decay.max(0.0) * dt).max(0.0);
    poise.knockback *= slowed / speed;
}

/// Drains the poise of each upright tank the tumbling `tank` runs into fast
/// enough, once per knockdown. Returns the tanks it has hit so far.
fn impact_tanks(scene: &mut Scene, tank: GameObject, poise: &ComponentPoise) -> Vec<GameObjectRef> {
    let mut impacted = poise.impacted.clone();
    let velocity = body_velocity(scene, tank);
    let speed = velocity.magnitude();
    if speed < poise.impact_min_speed {
        return impacted;
    }
    let position = scene.world_transform(tank).position;
    let others: Vec<_> = scene
        .objects_with::<ComponentPoise>()
        .filter(|&other| other != tank)
        .collect();
    for other in others {
        let id = scene.uuid(other);
        if impacted.iter().any(|hit| hit.id() == id) {
            continue;
        }
        let offset = scene.world_transform(other).position - position;
        if offset.magnitude() > poise.impact_radius {
            continue;
        }
        let mut direction = flatten_xz(velocity);
        if direction == Vec3::zeros() {
            direction = flatten_xz(offset);
        }
        let other_tuning = scene
            .read_component::<ComponentPoise, _, _>(other, Clone::clone)
            .unwrap_or_default();
        strike(
            scene,
            other,
            Blow {
                poise_damage: speed * poise.impact_poise_per_speed,
                direction,
                shove: speed * 0.5,
                launch: speed * 0.7,
                lift: other_tuning.knockdown_lift,
                spin: other_tuning.knockdown_spin,
                juggle: 0.0,
            },
        );
        crate::combat_vfx::spawn_impact(scene, position + offset * 0.5 + vec3(0.0, 0.8, 0.0), true);
        impacted.push(GameObjectRef::new(id));
    }
    impacted
}

/// The collider shapes the prototype tank prefab carries, for the generator
/// and for checks: (name, centre in the tank's frame, half extents).
pub const TANK_COLLIDERS: [(&str, Vec3, Vec3); 2] = [
    (
        "Hull Collider",
        Vec3::new(0.0, 0.66, -0.2),
        Vec3::new(1.1, 0.52, 1.35),
    ),
    (
        "Turret Collider",
        Vec3::new(0.0, 1.5, 0.05),
        Vec3::new(0.62, 0.32, 0.72),
    ),
];

/// A collider component for one of [`TANK_COLLIDERS`].
pub fn tank_collider(half_extents: Vec3) -> ComponentCollider {
    ComponentCollider {
        shape: ColliderShape::Cuboid { half_extents },
        friction: 0.7,
        density: 60.0,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
