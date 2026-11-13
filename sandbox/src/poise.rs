//! PROTOTYPE (poise_poc, #610): a Smash-style knockout meter, physics
//! knockdowns, knockout zones, a spring boot, a grappling hook and a few
//! more abilities, played in `assets/game/scenes/poise_poc.cxscene` to judge
//! whether knocking tanks around is fun. Nothing here is production code:
//! remove it, or rebuild it properly, once play-testing has decided.
//!
//! Everything is gated on the prototype's own components, so the arena and
//! the main game flow never meet it:
//!
//! - [`ComponentKnockout`] is a tank's knockout meter. Every hit adds to its
//!   percentage, which never caps, and knocks the tank back harder the
//!   higher it is. A hard enough hit launches the tank: its rigid body turns
//!   dynamic and tumbles, the player can steer it a little, and after the
//!   recovery time it rights itself and is briefly immune to launches. A
//!   tank that falls past the kill height or into a [`ComponentKnockoutZone`]
//!   is knocked out: it respawns at its start at 0%, invulnerable for a
//!   moment.
//! - [`ComponentDummyDriver`] drives a tank without input: a loop, not at
//!   all, or a simple bot that fights the player.
//! - [`ComponentTankAbilities`](abilities::ComponentTankAbilities) gives a
//!   tank the spring boot, the hook, an air boost, an anchor, bumper mines
//!   and oil slicks.
//! - [`ComponentPoiseHud`](hud::ComponentPoiseHud) draws the meters, the
//!   knockouts, the abilities and their tells.
//!
//! The tank controller asks [`tank_control`] who drives and [`traction`] how
//! much grip it has, and a round asks [`absorb_round`] whether the meter
//! takes it; all of them answer as before for a tank without these
//! components.

pub mod abilities;
pub mod bot;
pub mod hud;
pub mod logic;

use crate::tank::{flatten_xz, hull_obstacles, level_rotation, rotation_yaw, sweep_hull};
use calyx_engine::component::{
    ColliderShape, Component, ComponentCollider, ComponentEventContext, ComponentID, ComponentRef,
    ComponentRigidBody, ComponentTransform, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use logic::{knockback_speed, MeterState, MeterTuning, Reaction, Stance, Transition};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use rapier3d::prelude::{Collider, ColliderHandle, RigidBodyHandle, RigidBodyType};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// This far out, a tank has left the map whatever its height.
const LOST_BEYOND: f32 = 150.0;
/// The height of a tank's hull centre, where rounds hit it.
const HIT_CENTER_HEIGHT: f32 = 1.2;
/// The name of the child that holds a prototype tank's model.
const MODEL_NAME: &str = "Model";
/// How often an invulnerable tank blinks, per second.
const BLINK_RATE: f32 = 8.0;

/// PROTOTYPE (#610). A tank's knockout meter, which replaces its health in
/// the prototype scene, and every tuning value of hits, launches,
/// knockouts and respawns.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f302e8ac-c334-46dd-8339-6a7d878dc8e8"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Knockout Meter (prototype)")]
#[serde(default)]
pub struct ComponentKnockout {
    /// Percent a round adds to the meter.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub shot_damage: f32,
    /// A round's knockback at 0%, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub shot_knockback: f32,
    /// How much a round's knockback grows per percent on the meter.
    #[reflect_attr(min = 0.0, speed = 0.001)]
    pub shot_growth: f32,
    /// Knockback at or above this, in metres per second, launches the tank
    /// into a tumble; below it, the tank is only shoved.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub launch_threshold: f32,
    /// A launch's upward speed as a share of its horizontal speed.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub launch_lift: f32,
    /// Spin of a launch, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub launch_spin: f32,
    /// Share of a hit's knockback a tank already tumbling takes.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub juggle_share: f32,
    /// How fast a shove dies away, in metres per second squared.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub knockback_decay: f32,
    /// Gravity multiplier while tumbling; above 1 lands sooner and heavier.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub tumble_gravity_scale: f32,
    /// Linear damping while tumbling.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub tumble_linear_damping: f32,
    /// Angular damping while tumbling.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub tumble_angular_damping: f32,
    /// How hard the movement keys steer a tumbling tank, in metres per
    /// second squared.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub tumble_steering: f32,
    /// Seconds on physics before the tank rights itself.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub recovery_time: f32,
    /// Seconds the tank takes to right itself.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub righting_time: f32,
    /// How high the tank hops while righting itself, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub righting_hop: f32,
    /// Seconds after righting in which a hit cannot launch the tank.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub immunity_time: f32,
    /// A tank below this height is knocked out, in metres.
    #[reflect_attr(speed = 0.1)]
    pub kill_height: f32,
    /// Seconds from a knockout to the respawn.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub respawn_delay: f32,
    /// Seconds after a respawn in which hits do nothing.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub respawn_invulnerability: f32,
    /// A tumbling tank faster than this, in metres per second, hits an
    /// upright tank it runs into.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub impact_min_speed: f32,
    /// Percent a tumbling tank adds to a tank it runs into, per metre per
    /// second of its speed.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub impact_damage_per_speed: f32,
    /// How close, in metres, a tumbling tank's centre must come to another's
    /// to hit it.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub impact_radius: f32,
    /// How fast camera shake dies away, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub shake_decay: f32,

    #[serde(skip)]
    #[reflect_skip]
    pub(crate) state: MeterState,
    /// The shove a hit gave the upright tank, in metres per second.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) knockback: Vec3,
    /// Seconds since the last launch, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) since_launch: f32,
    /// The height the tank stood at when it went down, which the camera
    /// keeps while it tumbles.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) rest_height: f32,
    /// Seconds left of a hit-stop freeze.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hitstop: f32,
    /// A launch waiting for the hit-stop to end: velocity and spin.
    #[serde(skip)]
    #[reflect_skip]
    pending_launch: Option<(Vec3, f32)>,
    /// Camera shake, in metres, on the camera of the player in this tank.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) shake: f32,
    #[serde(skip)]
    #[reflect_skip]
    shake_phase: f32,
    /// Whether this tumble's air boost is spent.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boosted: bool,
    /// Whether the tank is on an oil slick.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) oiled: bool,
    /// The pose righting started from and the upright pose it ends at.
    #[serde(skip)]
    #[reflect_skip]
    righting: Option<(Transform, Transform)>,
    /// Tanks this tumble has already hit.
    #[serde(skip)]
    #[reflect_skip]
    impacted: Vec<GameObjectRef>,
    /// Where the tank started: where it respawns.
    #[serde(skip)]
    #[reflect_skip]
    home: Option<Transform>,
}

impl Default for ComponentKnockout {
    fn default() -> Self {
        Self {
            shot_damage: 5.0,
            shot_knockback: 2.5,
            shot_growth: 0.012,
            launch_threshold: 11.0,
            launch_lift: 0.4,
            launch_spin: 6.0,
            juggle_share: 0.5,
            knockback_decay: 22.0,
            tumble_gravity_scale: 1.6,
            tumble_linear_damping: 0.2,
            tumble_angular_damping: 0.5,
            tumble_steering: 7.0,
            recovery_time: 2.0,
            righting_time: 0.45,
            righting_hop: 0.9,
            immunity_time: 1.0,
            kill_height: -4.0,
            respawn_delay: 1.2,
            respawn_invulnerability: 2.0,
            impact_min_speed: 6.0,
            impact_damage_per_speed: 0.8,
            impact_radius: 2.6,
            shake_decay: 3.0,
            state: MeterState::default(),
            knockback: Vec3::zeros(),
            since_launch: f32::INFINITY,
            rest_height: 0.0,
            hitstop: 0.0,
            pending_launch: None,
            shake: 0.0,
            shake_phase: 0.0,
            boosted: false,
            oiled: false,
            righting: None,
            impacted: Vec::new(),
            home: None,
        }
    }
}

impl Component for ComponentKnockout {}

impl ComponentKnockout {
    pub(crate) fn tuning(&self) -> MeterTuning {
        MeterTuning {
            recovery_time: self.recovery_time,
            righting_time: self.righting_time,
            immunity_time: self.immunity_time,
            respawn_delay: self.respawn_delay,
            respawn_invulnerability: self.respawn_invulnerability,
        }
    }

    pub(crate) fn state(&self) -> &MeterState {
        &self.state
    }
}

/// PROTOTYPE (#610). A volume that knocks out any tank whose hull centre
/// enters it, such as the water channel: a box of `half_extents` around
/// the object, along the world's axes.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "12592b36-9c9d-404e-bc3f-1a8ff9557c1e"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Knockout Zone (prototype)")]
#[serde(default)]
pub struct ComponentKnockoutZone {
    pub half_extents: Vec3,
}

impl Default for ComponentKnockoutZone {
    fn default() -> Self {
        Self {
            half_extents: vec3(2.0, 2.0, 2.0),
        }
    }
}

impl Component for ComponentKnockoutZone {}

/// Whether `position` is inside a knockout zone.
pub(crate) fn in_knockout_zone(scene: &Scene, position: Vec3) -> bool {
    scene.objects_with::<ComponentKnockoutZone>().any(|zone| {
        let center = scene.world_transform(zone).position;
        scene
            .read_component::<ComponentKnockoutZone, _, _>(zone, |zone| {
                let offset = position - center;
                offset.x.abs() <= zone.half_extents.x
                    && offset.y.abs() <= zone.half_extents.y
                    && offset.z.abs() <= zone.half_extents.z
            })
            .unwrap_or(false)
    })
}

/// PROTOTYPE (#610). How a dummy drives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Reflect)]
#[uuid = "32e77f14-ca1a-4a34-a01f-c496bc48406d"]
pub enum DummyMode {
    /// Drives round in a loop.
    DriveLoop,
    /// Stays where it is.
    StayPut,
    /// Fights the player: chases it, keeps its distance, shoots, boots and
    /// boosts away from knockout edges.
    #[default]
    Bot,
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
    /// The distance the bot keeps from the player, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub bot_distance: f32,
    /// Seconds between the bot's shots.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub bot_fire_interval: f32,
    /// How far the bot shoots, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub bot_fire_range: f32,
    /// The bot boots the player within this distance, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub bot_boot_range: f32,

    #[serde(skip)]
    #[reflect_skip]
    pub(crate) fire_timer: f32,
}

impl Default for ComponentDummyDriver {
    fn default() -> Self {
        Self {
            mode: DummyMode::Bot,
            throttle: 0.6,
            steer: 0.35,
            bot_distance: 9.0,
            bot_fire_interval: 1.1,
            bot_fire_range: 16.0,
            bot_boot_range: 6.5,
            fire_timer: 0.0,
        }
    }
}

impl Component for ComponentDummyDriver {}

/// Who drives a tank this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TankControl {
    /// The player's input, as for every tank outside the prototype.
    Player,
    /// Nobody: the tank is down, hooked, out or frozen by a hit. The camera
    /// follows `camera_focus`.
    Locked { camera_focus: Vec3 },
    /// The dummy driver, with this throttle and steering.
    Dummy { throttle: f32, steer: f32 },
    /// The bot, with this throttle and steering, aiming at `aim`.
    Bot {
        throttle: f32,
        steer: f32,
        aim: Vec3,
    },
}

/// Who drives `tank` this frame: [`TankControl::Player`] for any tank
/// without the prototype's components.
pub fn tank_control(scene: &Scene, tank: GameObject) -> TankControl {
    let locked = scene
        .read_component::<ComponentKnockout, _, _>(tank, |knockout| {
            (!knockout.state.is_upright() || knockout.hitstop > 0.0).then_some(knockout.rest_height)
        })
        .flatten();
    if let Some(rest_height) = locked {
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
            DummyMode::Bot => bot::control(scene, tank, &driver),
        },
        None => TankControl::Player,
    }
}

/// Whether the bot driving `tank` fires this frame, counting its fire
/// interval down by `dt`.
pub fn bot_fires(scene: &mut Scene, tank: GameObject, dt: f32) -> bool {
    bot::fires(scene, tank, dt)
}

/// The share of its grip `tank` keeps: below 1 on an oil slick, 1 for any
/// other tank.
pub fn traction(scene: &Scene, tank: GameObject) -> f32 {
    if scene
        .read_component::<ComponentKnockout, _, _>(tank, |_| ())
        .is_none()
    {
        return 1.0;
    }
    abilities::oil_under(scene, scene.world_transform(tank).position)
        .map_or(1.0, |oil| oil.traction.clamp(0.0, 1.0))
}

/// Shakes the camera of the player in `tank` after it was placed this frame,
/// as hard as the tank's shake says. Nothing for any other tank.
pub fn shake_camera(scene: &mut Scene, tank: GameObject, camera: GameObjectRef) {
    let Some((shake, phase)) = scene
        .read_component::<ComponentKnockout, _, _>(tank, |knockout| {
            (knockout.shake, knockout.shake_phase)
        })
        .filter(|(shake, _)| *shake > 1e-3)
    else {
        return;
    };
    let Some(camera) = camera.game_object(scene) else {
        return;
    };
    let pose = scene.world_transform(camera);
    let offset = vec3(
        (phase * 53.0).sin() + (phase * 31.0).cos() * 0.5,
        0.0,
        (phase * 47.0).cos() + (phase * 29.0).sin() * 0.5,
    ) * shake;
    place(scene, camera, pose.position + offset, pose.rotation);
}

/// How far above a tank's origin rounds aim for: the middle of the hull of
/// a prototype tank, so rounds flying at gun height meet it. Zero for any
/// other target.
pub fn hit_center_offset(scene: &Scene, target: GameObject) -> Vec3 {
    if scene
        .read_component::<ComponentKnockout, _, _>(target, |_| ())
        .is_some()
    {
        Vec3::y() * HIT_CENTER_HEIGHT
    } else {
        Vec3::zeros()
    }
}

/// A hit on a tank's meter: what it adds and how hard it knocks back.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    /// Percent added to the meter.
    pub damage: f32,
    /// Knockback at 0%, in metres per second.
    pub knockback: f32,
    /// How much the knockback grows per percent on the meter.
    pub growth: f32,
    /// Horizontal unit direction of the knockback.
    pub direction: Vec3,
    /// Seconds both the hit tank and its attacker freeze for.
    pub hitstop: f32,
    /// Camera shake on the hit tank, in metres.
    pub shake: f32,
}

/// Lets `target`'s meter take a round of `_damage` flying along `direction`.
/// Returns false, doing nothing, for a target without a meter: its health
/// takes the round as before.
pub fn absorb_round(scene: &mut Scene, target: GameObject, _damage: f32, direction: Vec3) -> bool {
    let Some(knockout) = scene.read_component::<ComponentKnockout, _, _>(target, Clone::clone)
    else {
        return false;
    };
    let mut direction = flatten_xz(direction);
    if direction == Vec3::zeros() {
        direction = Vec3::z();
    }
    strike(
        scene,
        target,
        Hit {
            damage: knockout.shot_damage,
            knockback: knockout.shot_knockback,
            growth: knockout.shot_growth,
            direction,
            hitstop: 0.0,
            shake: 0.08,
        },
    );
    true
}

/// Deals `hit` to `target`: its meter goes up, and it is shoved, launched
/// into a tumble or pushed while tumbling, as hard as the meter says.
pub(crate) fn strike(scene: &mut Scene, target: GameObject, hit: Hit) -> Option<Reaction> {
    let (reaction, lift, spin, juggle) =
        scene.write_component::<ComponentKnockout, _>(target, |knockout| {
            let speed = knockback_speed(
                hit.knockback,
                hit.growth,
                knockout.state.percent + hit.damage.max(0.0),
            );
            let reaction = knockout
                .state
                .hit(hit.damage, speed, knockout.launch_threshold);
            if reaction != Reaction::Ignored {
                knockout.hitstop = knockout.hitstop.max(hit.hitstop);
                knockout.shake = knockout.shake.max(hit.shake);
            }
            if let Reaction::Shove(speed) = reaction {
                knockout.knockback += hit.direction * speed;
            }
            (
                reaction,
                knockout.launch_lift,
                knockout.launch_spin,
                knockout.juggle_share,
            )
        })?;
    match reaction {
        Reaction::Launch(speed) => {
            let velocity = hit.direction * speed + Vec3::y() * speed * lift;
            if hit.hitstop > 0.0 {
                let _ = scene.write_component::<ComponentKnockout, _>(target, |knockout| {
                    knockout.pending_launch = Some((velocity, spin));
                });
            } else {
                knock_down(scene, target, velocity, spin);
            }
        }
        Reaction::Juggle(speed) => {
            if let Some(body) = body_handle(scene, target) {
                let body = &mut scene.physics.bodies[body];
                let push = (hit.direction + Vec3::y() * lift) * speed * juggle;
                body.set_linvel(body.linvel() + push, true);
            }
        }
        Reaction::Shove(_) | Reaction::Ignored => {}
    }
    Some(reaction)
}

/// Launches `tank` into a tumble at `velocity` times its meter's growth,
/// whatever the launch threshold, adding `damage` to its meter. Nothing
/// happens to an invulnerable, anchored, righting or knocked-out tank but
/// the meter. Returns whether it was launched.
pub(crate) fn launch(
    scene: &mut Scene,
    tank: GameObject,
    velocity: Vec3,
    growth: f32,
    damage: f32,
) -> bool {
    let Some((launchable, scale, spin)) =
        scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
            let state = &mut knockout.state;
            if state.is_invulnerable() || state.is_knocked_out() {
                return (false, 0.0, 0.0);
            }
            state.percent += damage.max(0.0);
            let launchable = !state.is_anchored()
                && matches!(
                    state.stance,
                    Stance::Upright | Stance::KnockedDown { .. } | Stance::Hooked
                );
            (
                launchable,
                knockback_speed(1.0, growth, state.percent),
                knockout.launch_spin,
            )
        })
    else {
        return false;
    };
    if launchable {
        knock_down(scene, tank, velocity * scale, spin);
    }
    launchable
}

/// Puts `tank` down: its rigid body turns dynamic and flies off at
/// `velocity`, tumbling at `spin` radians per second about the horizontal
/// axis square to it, so it rolls over the way it was hit.
pub(crate) fn knock_down(scene: &mut Scene, tank: GameObject, velocity: Vec3, spin: f32) {
    let position = scene.world_transform(tank).position;
    let Some(knockout) = scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
        if knockout.state.is_upright() {
            knockout.rest_height = position.y;
        }
        knockout.state.knock_down();
        knockout.knockback = Vec3::zeros();
        knockout.since_launch = 0.0;
        knockout.righting = None;
        knockout.impacted.clear();
        knockout.boosted = false;
        knockout.pending_launch = None;
        knockout.clone()
    }) else {
        return;
    };
    set_body_type(scene, tank, RigidBodyType::Dynamic, &knockout);
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
pub(crate) fn set_body_type(
    scene: &mut Scene,
    tank: GameObject,
    ty: RigidBodyType,
    knockout: &ComponentKnockout,
) {
    let dynamic = ty == RigidBodyType::Dynamic;
    let gravity_scale = if dynamic {
        knockout.tumble_gravity_scale
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
        body.set_linear_damping(knockout.tumble_linear_damping);
        body.set_angular_damping(knockout.tumble_angular_damping);
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

/// Sets the friction of every collider on `tank` and its descendants: the
/// components' own, scaled by `scale`.
fn scale_friction(scene: &mut Scene, tank: GameObject, scale: f32) {
    let colliders: Vec<_> = scene
        .descendants(tank)
        .filter_map(|object| {
            scene
                .read_component::<ComponentCollider, _, _>(object, |collider| collider.friction)
                .map(|friction| (object.entity, friction))
        })
        .collect();
    let handles: Vec<_> = scene
        .physics
        .colliders
        .iter()
        .filter_map(|(handle, _)| {
            let entity = scene.physics.collider_entity(handle)?;
            colliders
                .iter()
                .find(|(owner, _)| *owner == entity)
                .map(|&(_, friction)| (handle, friction))
        })
        .collect();
    for (handle, friction) in handles {
        if let Some(collider) = scene.physics.colliders.get_mut(handle) {
            collider.set_friction(friction * scale);
        }
    }
}

/// The velocity of `tank`'s body: the physics' while it tumbles, zero while
/// kinematic.
pub(crate) fn body_velocity(scene: &Scene, tank: GameObject) -> Vec3 {
    body_handle(scene, tank)
        .and_then(|handle| scene.physics.bodies.get(handle))
        .filter(|body| body.is_dynamic())
        .map_or_else(Vec3::zeros, |body| *body.linvel())
}

/// Adds `delta` to the velocity of `tank`'s body while it tumbles.
pub(crate) fn push_body(scene: &mut Scene, tank: GameObject, delta: Vec3) {
    if let Some(handle) = body_handle(scene, tank) {
        let body = &mut scene.physics.bodies[handle];
        if body.is_dynamic() {
            body.set_linvel(body.linvel() + delta, true);
        }
    }
}

/// Whether the straight line from `from` to `to` is clear of every static
/// obstacle a hull would stop at, such as walls and cover.
pub(crate) fn line_clear(scene: &Scene, from: Vec3, to: Vec3) -> bool {
    let along = to - from;
    let distance = along.magnitude();
    distance <= f32::EPSILON || wall_distance(scene, from, along / distance, distance).is_none()
}

/// How far from `from` along the unit `direction`, within `max` metres, the
/// first static obstacle a hull would stop at is.
pub(crate) fn wall_distance(scene: &Scene, from: Vec3, direction: Vec3, max: f32) -> Option<f32> {
    let obstacles = hull_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    scene
        .physics
        .cast_ray(
            from,
            direction,
            max,
            true,
            crate::collision::HULL
                .query_filter()
                .predicate(&is_obstacle),
        )
        .map(|hit| hit.toi)
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
pub(crate) fn ground_height(scene: &Scene, position: Vec3) -> Option<f32> {
    let surfaces = crate::terrain::drive_surfaces(scene);
    crate::terrain::surface_height_below(scene, &surfaces, position + Vec3::y() * 3.0, 8.0)
}

/// Whether the scene has any ground to fall off: a scene without drive
/// surfaces, such as a test's, never drops its tanks.
pub(crate) fn has_ground(scene: &Scene) -> bool {
    !crate::terrain::drive_surfaces(scene).is_empty()
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

/// How far out a recovering tank is searched for a free spot, in metres,
/// and in what steps.
const FREE_SPOT_REACH: f32 = 8.0;
const FREE_SPOT_STEP: f32 = 0.25;
/// Extra clearance a recovering tank keeps from walls, in metres.
const FREE_SPOT_MARGIN: f32 = 0.05;

/// Whether an upright tank at `position` facing `rotation` overlaps no wall
/// or other blocking geometry: neither the box its hull drives with nor the
/// box it tumbles with.
pub(crate) fn hull_free(scene: &Scene, position: Vec3, rotation: UnitQuaternion<f32>) -> bool {
    let obstacles = hull_obstacles(scene);
    let is_obstacle = |collider: ColliderHandle, _: &Collider| {
        scene
            .physics
            .collider_entity(collider)
            .is_some_and(|entity| obstacles.contains(&entity))
    };
    let margin = Vec3::repeat(FREE_SPOT_MARGIN);
    let boxes = [
        (
            crate::tank::TANK_COLLIDER_OFFSET,
            crate::tank::TANK_COLLIDER_HALF_EXTENTS,
        ),
        (TANK_COLLIDERS[0].1, TANK_COLLIDERS[0].2),
    ];
    boxes.iter().all(|&(offset, half_extents)| {
        scene
            .physics
            .intersect_shape(
                ColliderShape::Cuboid {
                    half_extents: half_extents + margin,
                },
                position + rotation * offset,
                rotation,
                crate::collision::HULL
                    .query_filter()
                    .predicate(&is_obstacle),
            )
            .is_none()
    })
}

/// Where a tank lying at `pose` recovers to: upright on the ground under
/// it, or, when that would leave its hull inside a wall or other cover,
/// upright at the nearest free spot. It is never moved off the ground or
/// into a knockout zone, and on equal terms it moves towards `toward`, the
/// arena's playable side.
pub(crate) fn recovery_pose(
    scene: &Scene,
    pose: &Transform,
    rest_height: f32,
    toward: Vec3,
) -> (Vec3, UnitQuaternion<f32>) {
    let (position, rotation) = upright_pose(scene, pose, rest_height);
    let grounded = has_ground(scene);
    let height_at = |spot: Vec3| {
        if grounded {
            ground_height(scene, spot)
        } else {
            Some(rest_height)
        }
    };
    let free = |spot: Vec3| {
        let Some(height) = height_at(spot) else {
            return false;
        };
        let spot = vec3(spot.x, height, spot.z);
        hull_free(scene, spot, rotation) && !in_knockout_zone(scene, spot + Vec3::y() * 0.6)
    };
    // Right where it is when that is free, even at the edge of the ground.
    if hull_free(scene, position, rotation) {
        return (position, rotation);
    }
    match logic::nearest_free_spot(position, toward, FREE_SPOT_STEP, FREE_SPOT_REACH, free) {
        Some(spot) => {
            let height = height_at(spot).unwrap_or(position.y);
            (vec3(spot.x, height, spot.z), rotation)
        }
        None => (position, rotation),
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Shows or hides `tank`'s model.
fn show_model(scene: &mut Scene, tank: GameObject, visible: bool) {
    let model = scene
        .children(tank)
        .find(|&child| scene.name(child) == MODEL_NAME);
    if let Some(model) = model {
        let _ = scene.write_component::<ComponentID, _>(model, |id| {
            if id.visible != visible {
                id.visible = visible;
            }
        });
    }
}

/// Takes `tank` out: it stops, hides and waits for its respawn.
fn knock_out(scene: &mut Scene, tank: GameObject, knockout: &mut ComponentKnockout) {
    let position = scene.world_transform(tank).position;
    knockout.state.knock_out();
    knockout.knockback = Vec3::zeros();
    knockout.righting = None;
    knockout.pending_launch = None;
    knockout.hitstop = 0.0;
    knockout.shake = knockout.shake.max(0.4);
    set_body_type(scene, tank, RigidBodyType::KinematicPositionBased, knockout);
    show_model(scene, tank, false);
    let splash = vec3(position.x, position.y.max(knockout.kill_height), position.z);
    for offset in [Vec3::zeros(), vec3(0.6, 0.4, 0.0), vec3(-0.5, 0.2, 0.5)] {
        crate::combat_vfx::spawn_impact(scene, splash + offset, true);
    }
}

impl ComponentUpdate for ComponentKnockout {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        if crate::game_state::combat_paused(scene) {
            return;
        }
        let dt = resources.time().delta_time().max(0.0);
        let Some(mut knockout) = this.read(scene, Clone::clone) else {
            return;
        };
        let pose = scene.world_transform(game_object);
        let home = *knockout.home.get_or_insert(pose);
        knockout.since_launch += dt;
        knockout.shake = (knockout.shake - knockout.shake_decay.max(0.0) * dt).max(0.0);
        knockout.shake_phase += dt;

        // A hit-stop freezes the tank, and its launch waits for it to end.
        if knockout.hitstop > 0.0 {
            knockout.hitstop -= dt;
            if knockout.hitstop <= 0.0 {
                knockout.hitstop = 0.0;
                if let Some((velocity, spin)) = knockout.pending_launch.take() {
                    let _ = this.write(scene, |component| *component = knockout);
                    knock_down(scene, game_object, velocity, spin);
                    return;
                }
            }
            let _ = this.write(scene, |component| *component = knockout);
            return;
        }

        let tuning = knockout.tuning();
        let transition = knockout.state.tick(dt, &tuning);
        if transition == Some(Transition::Respawn) {
            // Back at the start, at 0%, blinking while invulnerable.
            place(scene, game_object, home.position, home.rotation);
            knockout.rest_height = home.position.y;
            knockout.knockback = Vec3::zeros();
            show_model(scene, game_object, true);
            let _ = this.write(scene, |component| *component = knockout);
            return;
        }

        // Out of the arena: past the kill height, into a knockout zone or
        // far out of the map.
        if !knockout.state.is_knocked_out() {
            let centre = pose.position + Vec3::y() * 0.6;
            let out = pose.position.y < knockout.kill_height
                || pose.position.x.abs() > LOST_BEYOND
                || pose.position.z.abs() > LOST_BEYOND
                || in_knockout_zone(scene, centre);
            if out {
                knock_out(scene, game_object, &mut knockout);
                let _ = this.write(scene, |component| *component = knockout);
                return;
            }
        }

        // Oil takes the grip of whatever drives or tumbles over it.
        let oil = abilities::oil_under(scene, pose.position);
        let oiled = oil.is_some();
        if oiled != knockout.oiled {
            let scale = oil.map_or(1.0, |oil| oil.tumble_friction);
            scale_friction(scene, game_object, scale);
            knockout.oiled = oiled;
        }

        match knockout.state.stance {
            Stance::Upright => {
                if transition == Some(Transition::Recovered) {
                    if let Some((_, upright)) = knockout.righting.take() {
                        place(scene, game_object, upright.position, upright.rotation);
                    }
                }
                // Driven off the ground, it falls.
                if has_ground(scene) && ground_height(scene, pose.position).is_none() {
                    let speed = scene
                        .read_component::<crate::tank::ComponentTankController, _, _>(
                            game_object,
                            |controller| controller.speed,
                        )
                        .unwrap_or(0.0);
                    let forward = flatten_xz(pose.rotation * Vec3::z());
                    let velocity = forward * speed + knockout.knockback;
                    let _ = this.write(scene, |component| *component = knockout.clone());
                    knock_down(scene, game_object, velocity, 1.5);
                    return;
                }
                let decay = if oiled {
                    knockout.knockback_decay * oil.map_or(1.0, |oil| oil.traction)
                } else {
                    knockout.knockback_decay
                };
                apply_knockback(scene, game_object, &mut knockout, dt, decay);
                // An invulnerable tank blinks.
                let blink = knockout.state.is_invulnerable()
                    && (knockout.state.invulnerable * BLINK_RATE).fract() < 0.5;
                show_model(scene, game_object, !blink);
            }
            Stance::KnockedDown { .. } => {
                // The player steers a tumbling tank a little.
                let driven_by_player = scene
                    .read_component::<ComponentDummyDriver, _, _>(game_object, |_| ())
                    .is_none();
                if driven_by_player && knockout.tumble_steering > 0.0 {
                    let steer = vec3(input.axis("move_right"), 0.0, input.axis("move_forward"));
                    if steer.magnitude_squared() > 1e-4 {
                        let steer = steer.normalize() * knockout.tumble_steering * dt;
                        push_body(scene, game_object, steer);
                    }
                }
                knockout.impacted = impact_tanks(scene, game_object, &knockout);
            }
            Stance::Righting { .. } => {
                if transition == Some(Transition::StartRighting) || knockout.righting.is_none() {
                    set_body_type(
                        scene,
                        game_object,
                        RigidBodyType::KinematicPositionBased,
                        &knockout,
                    );
                    // Clear of any wall it came to rest against or on.
                    let toward = home.position;
                    let (position, rotation) =
                        recovery_pose(scene, &pose, knockout.rest_height, toward);
                    let upright = Transform::from_components(position, rotation, pose.scale);
                    knockout.righting = Some((pose, upright));
                }
                if let Some((from, to)) = knockout.righting {
                    let t = smoothstep(knockout.state.righting_progress(&tuning));
                    let hop = (t * std::f32::consts::PI).sin() * knockout.righting_hop;
                    let position = from.position.lerp(&to.position, t) + Vec3::y() * hop;
                    let rotation = from.rotation.slerp(&to.rotation, t);
                    place(scene, game_object, position, rotation);
                }
            }
            Stance::Hooked | Stance::KnockedOut { .. } => {}
        }
        let _ = this.write(scene, |component| *component = knockout);
    }
}

/// Resets the prototype round in place: every tank with a meter back at its
/// spawn, upright, kinematic and at 0% with no knockouts, its abilities and
/// the bot's timers cleared, and every mine, oil slick and round in flight
/// gone. Nothing happens in a scene without the prototype's tanks.
pub fn reset_round(scene: &mut Scene) {
    let tanks: Vec<_> = scene.objects_with::<ComponentKnockout>().collect();
    if tanks.is_empty() {
        return;
    }
    let leftovers: Vec<_> = scene
        .objects_with::<abilities::ComponentBumperMine>()
        .chain(scene.objects_with::<abilities::ComponentOilSlick>())
        .chain(scene.objects_with::<crate::tank::ComponentProjectile>())
        .collect();
    for object in leftovers {
        scene.delete(object);
    }
    for tank in tanks {
        let Some(knockout) = scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
            // Keep the tuning and the spawn; drop every runtime state.
            *knockout = reset_runtime(knockout.clone());
            knockout.clone()
        }) else {
            continue;
        };
        set_body_type(
            scene,
            tank,
            RigidBodyType::KinematicPositionBased,
            &knockout,
        );
        scale_friction(scene, tank, 1.0);
        show_model(scene, tank, true);
        if let Some(home) = knockout.home {
            place(scene, tank, home.position, home.rotation);
        }
        let _ = scene.write_component::<abilities::ComponentTankAbilities, _>(tank, |abilities| {
            *abilities = abilities.reset();
        });
        let _ = scene.write_component::<ComponentDummyDriver, _>(tank, |driver| {
            driver.fire_timer = 0.0;
        });
        let _ =
            scene.write_component::<crate::tank::ComponentTankController, _>(tank, |controller| {
                controller.speed = 0.0;
                controller.ammo = controller.max_ammo;
                controller.fire_cooldown_remaining = 0.0;
                controller.reload_remaining = 0.0;
            });
    }
}

/// `knockout` with its tuning and home, and every runtime state as new.
fn reset_runtime(knockout: ComponentKnockout) -> ComponentKnockout {
    let fresh = ComponentKnockout::default();
    ComponentKnockout {
        state: fresh.state,
        knockback: fresh.knockback,
        since_launch: fresh.since_launch,
        rest_height: knockout.home.map_or(0.0, |home| home.position.y),
        hitstop: fresh.hitstop,
        pending_launch: fresh.pending_launch,
        shake: fresh.shake,
        shake_phase: fresh.shake_phase,
        boosted: fresh.boosted,
        oiled: fresh.oiled,
        righting: fresh.righting,
        impacted: fresh.impacted,
        ..knockout
    }
}

/// Moves the upright `tank` by the shove it was given, stopping at walls,
/// and lets the shove die away at `decay`.
fn apply_knockback(
    scene: &mut Scene,
    tank: GameObject,
    knockout: &mut ComponentKnockout,
    dt: f32,
    decay: f32,
) {
    let speed = knockout.knockback.magnitude();
    if speed <= 1e-3 {
        knockout.knockback = Vec3::zeros();
        return;
    }
    let mut transform = scene.world_transform(tank);
    transform.rotation = level_rotation(&transform.rotation);
    let (movement, contact) = sweep_hull(scene, &transform, knockout.knockback * dt);
    if movement != Vec3::zeros() {
        let pose = scene.world_transform(tank);
        place(scene, tank, pose.position + movement, pose.rotation);
    }
    if let Some(normal) = contact {
        // A wall takes the part of the shove pushing into it.
        let into = knockout.knockback.dot(&normal);
        if into < 0.0 {
            knockout.knockback -= normal * into;
        }
    }
    let slowed = (speed - decay.max(0.0) * dt).max(0.0);
    knockout.knockback *= slowed / speed;
}

/// Hits each upright tank the tumbling `tank` runs into fast enough, once
/// per tumble. Returns the tanks it has hit so far.
fn impact_tanks(
    scene: &mut Scene,
    tank: GameObject,
    knockout: &ComponentKnockout,
) -> Vec<GameObjectRef> {
    let mut impacted = knockout.impacted.clone();
    let velocity = body_velocity(scene, tank);
    let speed = velocity.magnitude();
    if speed < knockout.impact_min_speed {
        return impacted;
    }
    let position = scene.world_transform(tank).position;
    let others: Vec<_> = scene
        .objects_with::<ComponentKnockout>()
        .filter(|&other| other != tank)
        .collect();
    for other in others {
        let id = scene.uuid(other);
        if impacted.iter().any(|hit| hit.id() == id) {
            continue;
        }
        let offset = scene.world_transform(other).position - position;
        if offset.magnitude() > knockout.impact_radius {
            continue;
        }
        let mut direction = flatten_xz(velocity);
        if direction == Vec3::zeros() {
            direction = flatten_xz(offset);
        }
        strike(
            scene,
            other,
            Hit {
                damage: speed * knockout.impact_damage_per_speed,
                knockback: speed * 0.6,
                growth: knockout.shot_growth,
                direction,
                hitstop: 0.0,
                shake: 0.25,
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
