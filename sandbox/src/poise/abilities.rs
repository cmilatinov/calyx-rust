//! PROTOTYPE (poise_poc, #610): the tanks' abilities. A spring boot fired at
//! the cursor, a grappling hook swung by the turret, an air boost out of a
//! tumble, an anchor against knockback, bumper mines and oil slicks.

use super::logic::{boot_hit, bumper_launch, hook_lets_go, on_patch, Stance, Swing, SwingTuning};
use super::{
    body_velocity, bot, hull_blocked, knock_down, launch, line_clear, place, strike, wall_distance,
    ComponentDummyDriver, ComponentKnockout, Hit,
};
use crate::tank::{flatten_xz, ComponentTankController};
use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentID, ComponentMesh, ComponentRef, ComponentUpdate,
    ReflectComponent, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use serde::{Deserialize, Serialize};

/// The input action that fires the spring boot.
pub const BOOT_ACTION: &str = "boot";
/// The input action that fires, holds and releases the hook.
pub const HOOK_ACTION: &str = "hook";
/// The input action that air-boosts out of a tumble.
pub const BOOST_ACTION: &str = "boost";
/// The input action that drops anchor.
pub const ANCHOR_ACTION: &str = "anchor";
/// The input action that drops a bumper mine.
pub const MINE_ACTION: &str = "mine";
/// The input action that lays an oil slick.
pub const OIL_ACTION: &str = "oil";

/// How high above a tank's origin the hook line attaches, in metres.
const HOOK_HEIGHT: f32 = 1.0;
/// How close the flying hook must come to its target to latch, in metres.
const LATCH_DISTANCE: f32 = 0.8;
/// How high above the ground the boot's spring flies, in metres.
const BOOT_HEIGHT: f32 = 0.8;
/// How far in front of the tank's centre the spring is anchored, in metres.
const BOOT_ANCHOR: f32 = 1.2;
/// How long the boot holds at the end of its kick before springing back,
/// in seconds, so the kick reads.
const BOOT_HOLD: f32 = 0.12;
/// The radius a kick finds a tank within, around its centre, in metres.
const BOOT_TARGET_RADIUS: f32 = 1.2;
/// `game/meshes/arena_cube`, a unit cube.
const CUBE_MESH: &str = "4d2d87eb-60b4-4dcc-b3d4-67bc444396c9";
/// Sunny Battlefield's `hazard_yellow`, `hazard_black` and `metal`.
const YELLOW_MATERIAL: &str = "d7c59bc7-72c8-5803-b4d2-f957fedbd2a6";
const BLACK_MATERIAL: &str = "38294ccb-6ce1-55d1-a7cd-758e8daccc2f";
const METAL_MATERIAL: &str = "b5d9e6ac-69d2-5093-8e43-58e4d0a1c6c2";

/// What the hook is doing.
#[derive(Clone, Copy, Default)]
pub(crate) enum Hook {
    #[default]
    Idle,
    /// Flying out towards `target`, or towards `aim` when it has none.
    Flying {
        target: Option<GameObjectRef>,
        head: Vec3,
        aim: Vec3,
    },
    /// Latched on and swinging `target` round, for `elapsed` seconds.
    Latched {
        target: GameObjectRef,
        swing: Swing,
        elapsed: f32,
    },
    /// Coming back empty.
    Retracting { head: Vec3 },
}

/// What the spring boot is doing.
#[derive(Clone, Copy, Default)]
pub(crate) enum Boot {
    #[default]
    Idle,
    /// Compressing the spring before it fires, `elapsed` seconds in.
    WindingUp { elapsed: f32 },
    /// Springing out along `direction`, `extent` metres from the tank.
    Extending { direction: Vec3, extent: f32 },
    /// Springing back from `extent` metres out, once it has held there for
    /// `hold` more seconds.
    Retracting {
        direction: Vec3,
        extent: f32,
        hold: f32,
    },
}

/// How the HUD draws the boot this frame.
pub(crate) struct BootView {
    pub origin: Vec3,
    pub head: Vec3,
    pub direction: Vec3,
    /// How far through its wind-up the spring is, 0 to 1, while it winds up.
    pub winding: Option<f32>,
}

/// PROTOTYPE (#610). A tank's abilities, with their tuning.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5f74cef2-5b9d-46bd-922a-3b5a6f111a30"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Abilities (prototype)")]
#[serde(default)]
pub struct ComponentTankAbilities {
    /// Seconds the spring compresses before the boot fires.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub boot_windup: f32,
    /// How far the boot springs out from the front of the tank, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_range: f32,
    /// How fast the boot springs out, in metres per second.
    #[reflect_attr(min = 0.1, speed = 0.5)]
    pub boot_extend_speed: f32,
    /// How fast the boot springs back, in metres per second.
    #[reflect_attr(min = 0.1, speed = 0.5)]
    pub boot_retract_speed: f32,
    /// Radius of the boot's kicking end, in metres.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub boot_radius: f32,
    /// Percent a kick adds to the meter.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_damage: f32,
    /// A kick's knockback at 0%, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_knockback: f32,
    /// How much a kick's knockback grows per percent on the meter.
    #[reflect_attr(min = 0.0, speed = 0.001)]
    pub boot_growth: f32,
    /// Seconds both tanks freeze for when a kick lands.
    #[reflect_attr(min = 0.0, speed = 0.005)]
    pub boot_hitstop: f32,
    /// Camera shake when a kick lands, in metres.
    #[reflect_attr(min = 0.0, speed = 0.01)]
    pub boot_shake: f32,
    /// Seconds between boots, counted from the press.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub boot_cooldown: f32,
    /// How far the hook reaches, in metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub hook_range: f32,
    /// How near the aim point a tank must be for the hook to go for it, in
    /// metres.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub hook_aim_assist: f32,
    /// How fast the hook flies, in metres per second.
    #[reflect_attr(min = 0.1, speed = 0.5)]
    pub hook_speed: f32,
    /// Seconds before hooking again after a throw or a broken hook.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub hook_cooldown: f32,
    /// Seconds before hooking again after a miss.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub hook_miss_cooldown: f32,
    /// The longest a swing lasts, in seconds, before the line snaps and the
    /// hooked tank flies off as if released.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub hook_time_limit: f32,
    /// The radius the hook reels a tank in to, in metres; the line never
    /// gets longer than when it latched.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_radius: f32,
    /// How fast the hook reels a tank in to the swing radius, in metres per
    /// second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub reel_speed: f32,
    /// How hard the swing pulls the hooked tank towards where the turret
    /// points, in radians per second squared per radian it trails behind.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub swing_stiffness: f32,
    /// How much the swing's own speed is damped, per second; lower whips
    /// past the turret, higher settles on it.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub swing_damping: f32,
    /// The swing's top angular acceleration, in radians per second squared.
    #[reflect_attr(min = 0.0, speed = 0.5)]
    pub swing_max_acceleration: f32,
    /// The swing's top speed, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_max_speed: f32,
    /// How high a hooked tank is lifted while it swings, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_height: f32,
    /// The throw's speed as a share of the swing's speed at release.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub throw_strength: f32,
    /// Upward speed of a throw, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub throw_lift: f32,
    /// Percent a throw adds to the meter.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub throw_damage: f32,
    /// How much a throw's speed grows per percent on the meter.
    #[reflect_attr(min = 0.0, speed = 0.001)]
    pub throw_growth: f32,
    /// Spin of a throw, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub throw_spin: f32,
    /// The air boost's speed, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boost_speed: f32,
    /// The air boost's upward speed, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boost_lift: f32,
    /// Share of its velocity a boosting tank keeps.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub boost_carry: f32,
    /// Seconds between air boosts; one per tumble at most.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub boost_cooldown: f32,
    /// Seconds an anchor lasts: hits add to the meter but move nothing.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub anchor_duration: f32,
    /// Seconds between anchors.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub anchor_cooldown: f32,
    /// The most bumper mines a tank keeps out; another replaces the oldest.
    #[reflect_attr(min = 1.0, speed = 0.05)]
    pub mine_count: u32,
    /// Seconds before a mine can go off, even under its own tank.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub mine_arm_time: f32,
    /// How close a tank must come to a mine to set it off, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub mine_radius: f32,
    /// A mine's upward launch at 0%, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub mine_lift: f32,
    /// A mine's outward launch at 0%, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub mine_push: f32,
    /// How much a mine's launch grows per percent on the meter.
    #[reflect_attr(min = 0.0, speed = 0.001)]
    pub mine_growth: f32,
    /// Percent a mine adds to the meter.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub mine_damage: f32,
    /// Seconds between mines.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub mine_cooldown: f32,
    /// Half the side of an oil slick, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub oil_half_size: f32,
    /// Seconds an oil slick lasts.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub oil_lifetime: f32,
    /// Share of their grip tanks keep on oil.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub oil_traction: f32,
    /// Share of their friction tumbling tanks keep on oil.
    #[reflect_attr(min = 0.0, max = 1.0, speed = 0.01)]
    pub oil_tumble_friction: f32,
    /// Seconds between oil slicks.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub oil_cooldown: f32,

    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boot_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boost_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) anchor_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) mine_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) oil_cooldown_remaining: f32,
    /// Seconds since the last kick landed, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) since_kick: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boot: Boot,
    /// Where the boot's spring leaves the tank, and which way it points.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boot_origin: Vec3,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boot_aim: Vec3,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook: Hook,
    /// Where the hook line leaves the tank, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook_origin: Vec3,
    /// Whether the boost and anchor keys were held last frame: they are
    /// modifier keys, which report no presses of their own.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boost_held: bool,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) anchor_held: bool,
}

impl Default for ComponentTankAbilities {
    fn default() -> Self {
        Self {
            boot_windup: 0.2,
            boot_range: 9.0,
            boot_extend_speed: 34.0,
            boot_retract_speed: 16.0,
            boot_radius: 0.6,
            boot_damage: 16.0,
            boot_knockback: 11.0,
            boot_growth: 0.012,
            boot_hitstop: 0.07,
            boot_shake: 0.45,
            boot_cooldown: 1.5,
            hook_range: 18.0,
            hook_aim_assist: 5.0,
            hook_speed: 70.0,
            hook_cooldown: 3.0,
            hook_miss_cooldown: 0.8,
            hook_time_limit: 1.75,
            swing_radius: 5.5,
            reel_speed: 12.0,
            swing_stiffness: 60.0,
            swing_damping: 7.0,
            swing_max_acceleration: 70.0,
            swing_max_speed: 6.0,
            swing_height: 1.3,
            throw_strength: 0.85,
            throw_lift: 5.0,
            throw_damage: 12.0,
            throw_growth: 0.008,
            throw_spin: 8.0,
            boost_speed: 14.0,
            boost_lift: 6.0,
            boost_carry: 0.3,
            boost_cooldown: 4.0,
            anchor_duration: 1.0,
            anchor_cooldown: 5.0,
            mine_count: 2,
            mine_arm_time: 0.6,
            mine_radius: 1.5,
            mine_lift: 10.0,
            mine_push: 5.0,
            mine_growth: 0.01,
            mine_damage: 8.0,
            mine_cooldown: 1.5,
            oil_half_size: 2.75,
            oil_lifetime: 8.0,
            oil_traction: 0.15,
            oil_tumble_friction: 0.05,
            oil_cooldown: 6.0,
            boot_cooldown_remaining: 0.0,
            hook_cooldown_remaining: 0.0,
            boost_cooldown_remaining: 0.0,
            anchor_cooldown_remaining: 0.0,
            mine_cooldown_remaining: 0.0,
            oil_cooldown_remaining: 0.0,
            since_kick: f32::INFINITY,
            boot: Boot::Idle,
            boot_origin: Vec3::zeros(),
            boot_aim: Vec3::z(),
            hook: Hook::Idle,
            hook_origin: Vec3::zeros(),
            boost_held: false,
            anchor_held: false,
        }
    }
}

impl Component for ComponentTankAbilities {}

impl ComponentTankAbilities {
    fn swing_tuning(&self) -> SwingTuning {
        SwingTuning {
            radius: self.swing_radius,
            reel_speed: self.reel_speed,
            stiffness: self.swing_stiffness,
            damping: self.swing_damping,
            max_acceleration: self.swing_max_acceleration,
            max_speed: self.swing_max_speed,
        }
    }

    /// The far end of the hook line, while there is a line.
    pub(crate) fn hook_end(&self, scene: &Scene) -> Option<Vec3> {
        match self.hook {
            Hook::Idle => None,
            Hook::Flying { head, .. } | Hook::Retracting { head } => Some(head),
            Hook::Latched { target, .. } => target
                .game_object(scene)
                .map(|target| scene.world_transform(target).position + Vec3::y() * HOOK_HEIGHT),
        }
    }

    pub(crate) fn hook_latched(&self) -> bool {
        matches!(self.hook, Hook::Latched { .. })
    }

    /// How much of the swing's time is left, 1 to 0, while a tank is hooked.
    pub(crate) fn hook_time_left(&self) -> Option<f32> {
        match self.hook {
            Hook::Latched { elapsed, .. } if self.hook_time_limit > 0.0 => {
                Some((1.0 - elapsed / self.hook_time_limit).clamp(0.0, 1.0))
            }
            Hook::Latched { .. } => Some(0.0),
            _ => None,
        }
    }

    /// The boot, while it is out or winding up.
    pub(crate) fn boot_view(&self) -> Option<BootView> {
        let (direction, extent, winding) = match self.boot {
            Boot::Idle => return None,
            Boot::WindingUp { elapsed } => (
                self.boot_aim,
                0.0,
                Some((elapsed / self.boot_windup.max(1e-3)).clamp(0.0, 1.0)),
            ),
            Boot::Extending { direction, extent }
            | Boot::Retracting {
                direction, extent, ..
            } => (direction, extent, None),
        };
        Some(BootView {
            origin: self.boot_origin,
            head: self.boot_origin + direction * extent,
            direction,
            winding,
        })
    }
}

/// What a tank's abilities are asked for this frame, by the player's keys or
/// by the bot.
#[derive(Clone, Copy, Default)]
struct Requests {
    boot: bool,
    hook_held: bool,
    hook_pressed: bool,
    boost: bool,
    anchor: bool,
    mine: bool,
    oil: bool,
    /// Which way the movement keys point, for the air boost.
    steer: Vec3,
}

impl ComponentUpdate for ComponentTankAbilities {
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
        let Some(mut abilities) = this.read(scene, |abilities| *abilities) else {
            return;
        };
        let requests = match scene
            .read_component::<ComponentDummyDriver, _, _>(game_object, |driver| driver.mode)
        {
            Some(super::DummyMode::Bot) => bot_requests(scene, game_object, &abilities),
            Some(_) => return,
            None => {
                let boost_held = input.action(BOOST_ACTION).pressed();
                let anchor_held = input.action(ANCHOR_ACTION).pressed();
                let requests = Requests {
                    boot: input.action(BOOT_ACTION).just_pressed(),
                    hook_held: input.action(HOOK_ACTION).pressed(),
                    hook_pressed: input.action(HOOK_ACTION).just_pressed(),
                    boost: (boost_held && !abilities.boost_held)
                        || input.action(BOOST_ACTION).just_pressed(),
                    anchor: (anchor_held && !abilities.anchor_held)
                        || input.action(ANCHOR_ACTION).just_pressed(),
                    mine: input.action(MINE_ACTION).just_pressed(),
                    oil: input.action(OIL_ACTION).just_pressed(),
                    steer: vec3(input.axis("move_right"), 0.0, input.axis("move_forward")),
                };
                abilities.boost_held = boost_held;
                abilities.anchor_held = anchor_held;
                requests
            }
        };
        for cooldown in [
            &mut abilities.boot_cooldown_remaining,
            &mut abilities.hook_cooldown_remaining,
            &mut abilities.boost_cooldown_remaining,
            &mut abilities.anchor_cooldown_remaining,
            &mut abilities.mine_cooldown_remaining,
            &mut abilities.oil_cooldown_remaining,
        ] {
            *cooldown = (*cooldown - dt).max(0.0);
        }
        abilities.since_kick += dt;
        abilities.hook_origin = hook_origin(scene, game_object);

        let stance = scene
            .read_component::<ComponentKnockout, _, _>(game_object, |knockout| {
                knockout.state.stance
            })
            .unwrap_or_default();
        if stance != Stance::Upright {
            // Down or out: the boot springs back and the hook lets go; only
            // the air boost works.
            if let Boot::WindingUp { .. } = abilities.boot {
                abilities.boot = Boot::Idle;
            }
            if let Boot::Extending { direction, extent } = abilities.boot {
                abilities.boot = Boot::Retracting {
                    direction,
                    extent,
                    hold: 0.0,
                };
            }
            update_boot(scene, game_object, &mut abilities, dt);
            release_hook(scene, &mut abilities);
            if requests.boost && matches!(stance, Stance::KnockedDown { .. }) {
                air_boost(scene, game_object, &mut abilities, requests.steer);
            }
            let _ = this.write(scene, |component| *component = abilities);
            return;
        }

        if requests.boot
            && abilities.boot_cooldown_remaining <= 0.0
            && matches!(abilities.boot, Boot::Idle)
        {
            abilities.boot = Boot::WindingUp { elapsed: 0.0 };
            abilities.boot_cooldown_remaining = abilities.boot_cooldown;
        }
        update_boot(scene, game_object, &mut abilities, dt);
        update_hook(
            scene,
            game_object,
            &mut abilities,
            requests.hook_held,
            requests.hook_pressed,
            dt,
        );
        if requests.anchor && abilities.anchor_cooldown_remaining <= 0.0 {
            let duration = abilities.anchor_duration;
            let _ = scene.write_component::<ComponentKnockout, _>(game_object, |knockout| {
                knockout.state.anchored = duration;
            });
            abilities.anchor_cooldown_remaining = abilities.anchor_cooldown;
        }
        if requests.mine && abilities.mine_cooldown_remaining <= 0.0 {
            drop_mine(scene, game_object, &abilities);
            abilities.mine_cooldown_remaining = abilities.mine_cooldown;
        }
        if requests.oil && abilities.oil_cooldown_remaining <= 0.0 {
            lay_oil(scene, game_object, &abilities);
            abilities.oil_cooldown_remaining = abilities.oil_cooldown;
        }
        let _ = this.write(scene, |component| *component = abilities);
    }
}

/// What the bot asks of its abilities: the boot when the player is close and
/// in sight, and the air boost when it is about to fall off.
fn bot_requests(scene: &Scene, tank: GameObject, abilities: &ComponentTankAbilities) -> Requests {
    let mut requests = Requests::default();
    let Some(target) = bot::player(scene, tank) else {
        return requests;
    };
    let range = scene
        .read_component::<ComponentDummyDriver, _, _>(tank, |driver| driver.bot_boot_range)
        .unwrap_or(0.0);
    let from = scene.world_transform(tank).position;
    let to = scene.world_transform(target).position;
    requests.boot = abilities.boot_cooldown_remaining <= 0.0
        && (to - from).magnitude() <= range
        && line_clear(scene, from + Vec3::y(), to + Vec3::y());
    let velocity = body_velocity(scene, tank);
    requests.boost = bot::about_to_fall(scene, tank, velocity);
    // Back towards the middle of the arena.
    requests.steer = flatten_xz(-from);
    requests
}

/// Where the hook line leaves `tank`: its gun's muzzle, or above its centre.
fn hook_origin(scene: &Scene, tank: GameObject) -> Vec3 {
    let barrel = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            (controller.barrel, controller.muzzle_offset)
        })
        .and_then(|(barrel, offset)| barrel.game_object(scene).map(|barrel| (barrel, offset)));
    match barrel {
        Some((barrel, offset)) => {
            let transform = scene.world_transform(barrel);
            transform.position + flatten_xz(transform.forward()) * offset
        }
        None => scene.world_transform(tank).position + Vec3::y() * HOOK_HEIGHT,
    }
}

/// The point `tank` aims at: the player's crosshair, or for the bot the
/// player it fights, or straight ahead.
fn aim_point(scene: &Scene, tank: GameObject, range: f32) -> Vec3 {
    let pose = scene.world_transform(tank);
    if bot::is_bot(scene, tank) {
        if let Some(target) = bot::player(scene, tank) {
            return scene.world_transform(target).position;
        }
    }
    scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| controller.crosshair)
        .and_then(|crosshair| crosshair.game_object(scene))
        .map(|crosshair| scene.world_transform(crosshair).position)
        .unwrap_or_else(|| pose.position + flatten_xz(pose.rotation * Vec3::z()) * range)
}

/// Where the boot's spring is anchored on `tank` when it points along
/// `direction`.
fn boot_anchor(scene: &Scene, tank: GameObject, direction: Vec3) -> Vec3 {
    scene.world_transform(tank).position + direction * BOOT_ANCHOR + Vec3::y() * BOOT_HEIGHT
}

/// The way the boot points from `tank`: at the cursor, or straight ahead.
fn boot_direction(scene: &Scene, tank: GameObject, range: f32) -> Vec3 {
    let center = scene.world_transform(tank).position;
    let direction = flatten_xz(aim_point(scene, tank, range) - center);
    if direction == Vec3::zeros() {
        flatten_xz(scene.world_transform(tank).rotation * Vec3::z())
    } else {
        direction
    }
}

/// The hit a kick deals.
pub(crate) fn kick(abilities: &ComponentTankAbilities, direction: Vec3) -> Hit {
    Hit {
        damage: abilities.boot_damage,
        knockback: abilities.boot_knockback,
        growth: abilities.boot_growth,
        direction,
        hitstop: abilities.boot_hitstop,
        shake: abilities.boot_shake,
    }
}

/// Winds the boot up, springs it out or back by a frame, kicking the first
/// tank with a meter it reaches and springing back from a wall.
fn update_boot(
    scene: &mut Scene,
    tank: GameObject,
    abilities: &mut ComponentTankAbilities,
    dt: f32,
) {
    match abilities.boot {
        Boot::Idle => {}
        Boot::WindingUp { elapsed } => {
            // The spring compresses, still following the aim, then fires.
            let direction = boot_direction(scene, tank, abilities.boot_range);
            abilities.boot_aim = direction;
            abilities.boot_origin = boot_anchor(scene, tank, direction);
            let elapsed = elapsed + dt;
            abilities.boot = if elapsed >= abilities.boot_windup {
                Boot::Extending {
                    direction,
                    extent: 0.0,
                }
            } else {
                Boot::WindingUp { elapsed }
            };
        }
        Boot::Extending { direction, extent } => {
            let origin = boot_anchor(scene, tank, direction);
            abilities.boot_origin = origin;
            let to = (extent + abilities.boot_extend_speed.max(0.1) * dt).min(abilities.boot_range);
            let others: Vec<_> = scene
                .objects_with::<ComponentKnockout>()
                .filter(|&other| other != tank)
                .collect();
            let targets: Vec<_> = others
                .iter()
                .map(|&other| (scene.world_transform(other).position, BOOT_TARGET_RADIUS))
                .collect();
            let hit = boot_hit(
                origin,
                direction,
                extent,
                to,
                abilities.boot_radius,
                &targets,
            );
            // A wall stops the boot short of anything behind it.
            let wall = wall_distance(scene, origin, direction, to);
            abilities.boot = match (hit, wall) {
                (Some((index, at)), wall) if wall.is_none_or(|wall| at <= wall) => {
                    land_kick(scene, tank, others[index], abilities, origin, direction, at);
                    Boot::Retracting {
                        direction,
                        extent: at,
                        hold: BOOT_HOLD,
                    }
                }
                (_, Some(wall)) => {
                    crate::combat_vfx::spawn_impact(scene, origin + direction * wall, false);
                    Boot::Retracting {
                        direction,
                        extent: wall,
                        hold: BOOT_HOLD,
                    }
                }
                _ if to >= abilities.boot_range => Boot::Retracting {
                    direction,
                    extent: to,
                    hold: BOOT_HOLD,
                },
                _ => Boot::Extending {
                    direction,
                    extent: to,
                },
            };
        }
        Boot::Retracting {
            direction,
            extent,
            hold,
        } => {
            abilities.boot_origin = boot_anchor(scene, tank, direction);
            abilities.boot = if hold > 0.0 {
                Boot::Retracting {
                    direction,
                    extent,
                    hold: hold - dt,
                }
            } else {
                let extent = extent - abilities.boot_retract_speed.max(0.1) * dt;
                if extent <= 0.0 {
                    Boot::Idle
                } else {
                    Boot::Retracting {
                        direction,
                        extent,
                        hold: 0.0,
                    }
                }
            };
        }
    }
}

/// A kick lands on `target`: the hit, a hit-stop and shake on both tanks,
/// and a burst at the impact.
fn land_kick(
    scene: &mut Scene,
    tank: GameObject,
    target: GameObject,
    abilities: &mut ComponentTankAbilities,
    origin: Vec3,
    direction: Vec3,
    at: f32,
) {
    strike(scene, target, kick(abilities, direction));
    let hitstop = abilities.boot_hitstop;
    let shake = abilities.boot_shake * 0.8;
    let _ = scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
        knockout.hitstop = knockout.hitstop.max(hitstop);
        knockout.shake = knockout.shake.max(shake);
    });
    abilities.since_kick = 0.0;
    let point = origin + direction * (at + abilities.boot_radius);
    let side = vec3(-direction.z, 0.0, direction.x);
    for offset in [
        Vec3::zeros(),
        side * 0.5 + Vec3::y() * 0.3,
        -side * 0.5 + Vec3::y() * 0.2,
        direction * 0.6 + Vec3::y() * 0.5,
    ] {
        crate::combat_vfx::spawn_impact(scene, point + offset, true);
    }
    crate::combat_vfx::spawn_impact(scene, point - Vec3::y() * 0.6, false);
}

/// A burst out of a tumble, once per tumble: along the movement keys, or at
/// the cursor when none is held.
fn air_boost(
    scene: &mut Scene,
    tank: GameObject,
    abilities: &mut ComponentTankAbilities,
    steer: Vec3,
) {
    if abilities.boost_cooldown_remaining > 0.0 {
        return;
    }
    let spent = scene
        .read_component::<ComponentKnockout, _, _>(tank, |knockout| knockout.boosted)
        .unwrap_or(true);
    if spent {
        return;
    }
    let mut direction = flatten_xz(steer);
    if direction == Vec3::zeros() {
        direction = flatten_xz(aim_point(scene, tank, 1.0) - scene.world_transform(tank).position);
    }
    let velocity = body_velocity(scene, tank) * abilities.boost_carry.clamp(0.0, 1.0)
        + direction * abilities.boost_speed
        + Vec3::y() * abilities.boost_lift;
    if let Some(handle) = super::body_handle(scene, tank) {
        scene.physics.bodies[handle].set_linvel(velocity, true);
    }
    let _ = scene.write_component::<ComponentKnockout, _>(tank, |knockout| {
        knockout.boosted = true;
    });
    abilities.boost_cooldown_remaining = abilities.boost_cooldown;
    let position = scene.world_transform(tank).position;
    crate::combat_vfx::spawn_impact(scene, position - direction * 1.2 + Vec3::y() * 0.6, false);
    crate::combat_vfx::spawn_impact(scene, position - direction * 1.6 + Vec3::y() * 0.4, true);
}

/// The tank with a meter nearest `aim` within the hook's aim assist and
/// range of `tank`, that the hook could latch onto now.
fn hook_target(
    scene: &Scene,
    tank: GameObject,
    origin: Vec3,
    aim: Vec3,
    abilities: &ComponentTankAbilities,
) -> Option<GameObject> {
    let center = scene.world_transform(tank).position;
    scene
        .objects_with::<ComponentKnockout>()
        .filter(|&other| other != tank)
        .filter_map(|other| {
            let position = scene.world_transform(other).position;
            let hookable = scene
                .read_component::<ComponentKnockout, _, _>(other, |knockout| {
                    let state = &knockout.state;
                    !state.is_immune()
                        && !state.is_invulnerable()
                        && !state.is_anchored()
                        && matches!(state.stance, Stance::Upright | Stance::KnockedDown { .. })
                })
                .unwrap_or(false);
            let to_aim = (position - aim).xz().magnitude();
            (hookable
                && to_aim <= abilities.hook_aim_assist
                && (position - center).xz().magnitude() <= abilities.hook_range
                && line_clear(scene, origin, position + Vec3::y() * HOOK_HEIGHT))
            .then_some((other, to_aim))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(other, _)| other)
}

/// The angle `tank`'s turret points at on the ground, from +X towards +Z,
/// in radians, as a swing measures it.
fn turret_angle(scene: &Scene, tank: GameObject) -> Option<f32> {
    let turret = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| controller.turret)?
        .game_object(scene)?;
    let forward = flatten_xz(scene.world_transform(turret).rotation * Vec3::z());
    (forward != Vec3::zeros()).then(|| forward.z.atan2(forward.x))
}

/// Throws the hooked `target` along its swing, harder the higher its meter.
fn throw(
    scene: &mut Scene,
    target: GameObject,
    swing: &Swing,
    abilities: &mut ComponentTankAbilities,
) {
    let velocity = swing.throw_velocity(abilities.throw_strength, abilities.throw_lift);
    if !launch(
        scene,
        target,
        velocity,
        abilities.throw_growth,
        abilities.throw_damage,
    ) {
        // Somehow untouchable now: drop it where it is.
        knock_down(scene, target, Vec3::zeros(), 0.0);
    }
    abilities.hook = Hook::Idle;
    abilities.hook_cooldown_remaining = abilities.hook_cooldown;
}

fn update_hook(
    scene: &mut Scene,
    tank: GameObject,
    abilities: &mut ComponentTankAbilities,
    held: bool,
    pressed: bool,
    dt: f32,
) {
    let origin = abilities.hook_origin;
    let center = scene.world_transform(tank).position;
    match abilities.hook {
        Hook::Idle => {
            if pressed && abilities.hook_cooldown_remaining <= 0.0 {
                let aim = aim_point(scene, tank, abilities.hook_range);
                let target = hook_target(scene, tank, origin, aim, abilities);
                // A miss flies to the aim point, or to the end of the range.
                let reach = flatten_xz(aim - center) * abilities.hook_range;
                let aim = if (aim - center).xz().magnitude() <= abilities.hook_range {
                    aim + Vec3::y() * HOOK_HEIGHT
                } else {
                    center + reach + Vec3::y() * HOOK_HEIGHT
                };
                abilities.hook = Hook::Flying {
                    target: target.map(|target| GameObjectRef::new(scene.uuid(target))),
                    head: origin,
                    aim,
                };
            }
        }
        Hook::Flying { target, head, aim } => {
            let goal = target
                .and_then(|target| target.game_object(scene))
                .map(|target| scene.world_transform(target).position + Vec3::y() * HOOK_HEIGHT);
            let end = goal.unwrap_or(aim);
            let step = abilities.hook_speed.max(0.1) * dt;
            let to_end = end - head;
            let head = if to_end.magnitude() <= step {
                end
            } else {
                head + to_end.normalize() * step
            };
            let out_of_range = (head - center).xz().magnitude() > abilities.hook_range * 1.25;
            if !held || out_of_range || !line_clear(scene, origin, head) {
                abilities.hook = Hook::Retracting { head };
                abilities.hook_cooldown_remaining = abilities.hook_miss_cooldown;
            } else if let (Some(target), Some(goal)) = (target, goal) {
                if (goal - head).magnitude() <= LATCH_DISTANCE {
                    latch(scene, tank, abilities, target);
                } else {
                    abilities.hook = Hook::Flying {
                        target: Some(target),
                        head,
                        aim,
                    };
                }
            } else if (end - head).magnitude() <= 1e-3 {
                abilities.hook = Hook::Retracting { head };
                abilities.hook_cooldown_remaining = abilities.hook_miss_cooldown;
            } else {
                abilities.hook = Hook::Flying { target, head, aim };
            }
        }
        Hook::Retracting { head } => {
            let step = abilities.hook_speed.max(0.1) * 1.5 * dt;
            let back = origin - head;
            abilities.hook = if back.magnitude() <= step {
                Hook::Idle
            } else {
                Hook::Retracting {
                    head: head + back.normalize() * step,
                }
            };
        }
        Hook::Latched {
            target,
            mut swing,
            elapsed,
        } => {
            let Some(target_object) = target.game_object(scene) else {
                abilities.hook = Hook::Idle;
                return;
            };
            let still_hooked = scene
                .read_component::<ComponentKnockout, _, _>(target_object, |knockout| {
                    knockout.state.stance == Stance::Hooked
                })
                .unwrap_or(false);
            if !still_hooked {
                abilities.hook = Hook::Idle;
                return;
            }
            let elapsed = elapsed + dt;
            if hook_lets_go(held, elapsed, abilities.hook_time_limit) {
                // Let go, or the line snapped: thrown along the swing.
                throw(scene, target_object, &swing, abilities);
                return;
            }
            let from = scene.world_transform(target_object);
            // The hooked tank chases the way the turret points, round the
            // player, as fast as the turret turns it.
            let target_angle = turret_angle(scene, tank).unwrap_or(swing.angle);
            swing.follow(target_angle, dt, &abilities.swing_tuning());
            let ground = center.y;
            let mut position = center + swing.offset();
            position.y = ground + abilities.swing_height;
            // A wall in the way of the swing or the line breaks the hook,
            // and the hooked tank slams into it. The tank swings high
            // enough to clear low cover, not walls.
            if hull_blocked(scene, &from, position)
                || !line_clear(scene, origin, position + Vec3::y() * HOOK_HEIGHT * 0.5)
            {
                knock_down(
                    scene,
                    target_object,
                    swing.velocity(),
                    abilities.throw_spin * 0.5,
                );
                abilities.hook = Hook::Idle;
                abilities.hook_cooldown_remaining = abilities.hook_cooldown;
                return;
            }
            // Nose along the swing, banked into it.
            let tangent = flatten_xz(swing.velocity());
            let yaw = if tangent == Vec3::zeros() {
                crate::tank::rotation_yaw(&from.rotation)
            } else {
                tangent.x.atan2(tangent.z)
            };
            let bank = (swing.angular_speed * 0.08).clamp(-0.5, 0.5);
            let rotation = UnitQuaternion::from_euler_angles(0.0, yaw, 0.0)
                * UnitQuaternion::from_euler_angles(0.0, 0.0, bank);
            place(scene, target_object, position, rotation);
            abilities.hook = Hook::Latched {
                target,
                swing,
                elapsed,
            };
        }
    }
}

/// Latches the hook onto `target`: it leaves physics, if it was tumbling,
/// and starts its swing round `tank` at rest.
fn latch(
    scene: &mut Scene,
    tank: GameObject,
    abilities: &mut ComponentTankAbilities,
    target: GameObjectRef,
) {
    let Some(target_object) = target.game_object(scene) else {
        abilities.hook = Hook::Idle;
        return;
    };
    let height = scene.world_transform(target_object).position.y;
    let Some(knockout) = scene.write_component::<ComponentKnockout, _>(target_object, |knockout| {
        if knockout.state.is_upright() {
            knockout.rest_height = height;
        }
        knockout.state.stance = Stance::Hooked;
        knockout.knockback = Vec3::zeros();
        knockout.clone()
    }) else {
        abilities.hook = Hook::Idle;
        return;
    };
    super::set_body_type(
        scene,
        target_object,
        rapier3d::prelude::RigidBodyType::KinematicPositionBased,
        &knockout,
    );
    let center = scene.world_transform(tank).position;
    let position = scene.world_transform(target_object).position;
    abilities.hook = Hook::Latched {
        target,
        swing: Swing::starting_at(center, position),
        elapsed: 0.0,
    };
    crate::combat_vfx::spawn_impact(scene, position + Vec3::y() * HOOK_HEIGHT, true);
}

/// Lets go of whatever the hook holds: a held tank drops, tumbling, as it
/// was going.
fn release_hook(scene: &mut Scene, abilities: &mut ComponentTankAbilities) {
    match abilities.hook {
        Hook::Latched { target, swing, .. } => {
            if let Some(target) = target.game_object(scene) {
                knock_down(scene, target, swing.velocity(), abilities.throw_spin * 0.5);
            }
            abilities.hook = Hook::Idle;
            abilities.hook_cooldown_remaining = abilities.hook_cooldown;
        }
        Hook::Flying { head, .. } => abilities.hook = Hook::Retracting { head },
        Hook::Idle | Hook::Retracting { .. } => {}
    }
}

/// A child of `parent` drawing a unit cube at `size`, `offset` above it.
fn cube(
    scene: &mut Scene,
    parent: GameObject,
    name: &str,
    offset: Vec3,
    size: Vec3,
    material: &str,
) {
    let object = scene.create(
        Some(ComponentID {
            name: name.into(),
            ..Default::default()
        }),
        Some(parent),
    );
    let pose = scene.world_transform(parent);
    scene.set_world_transform(
        object,
        Transform::from_components(pose.position + pose.rotation * offset, pose.rotation, size)
            .matrix(),
    );
    scene.add_component(
        object,
        ComponentMesh {
            mesh: AssetRef::from_id(CUBE_MESH.parse().expect("cube mesh id")),
            material: AssetRef::from_id(material.parse().expect("material id")),
            cast_shadows: false,
        },
    );
}

/// A new root object named `name` at `position` turned by `rotation`.
fn spawn(
    scene: &mut Scene,
    name: &str,
    position: Vec3,
    rotation: UnitQuaternion<f32>,
) -> GameObject {
    let object = scene.create(
        Some(ComponentID {
            name: name.into(),
            ..Default::default()
        }),
        None,
    );
    scene.set_world_transform(
        object,
        Transform::from_components(position, rotation, vec3(1.0, 1.0, 1.0)).matrix(),
    );
    object
}

/// Drops a bumper mine behind `tank`, taking away its oldest mine when it
/// already has as many as it may.
fn drop_mine(scene: &mut Scene, tank: GameObject, abilities: &ComponentTankAbilities) {
    let owner = scene.uuid(tank);
    let mut mines: Vec<_> = scene
        .objects_with::<ComponentBumperMine>()
        .filter_map(|mine| {
            scene
                .read_component::<ComponentBumperMine, _, _>(mine, |bumper| {
                    (bumper.owner.id() == owner).then_some((mine, bumper.age))
                })
                .flatten()
        })
        .collect();
    mines.sort_by(|a, b| b.1.total_cmp(&a.1));
    while mines.len() >= abilities.mine_count.max(1) as usize {
        let (oldest, _) = mines.remove(0);
        scene.delete(oldest);
    }
    let pose = scene.world_transform(tank);
    let back = flatten_xz(pose.rotation * Vec3::z());
    let yaw =
        UnitQuaternion::from_euler_angles(0.0, crate::tank::rotation_yaw(&pose.rotation), 0.0);
    let mut position = pose.position - back * 2.4;
    position.y = super::ground_height(scene, position).unwrap_or(pose.position.y);
    let mine = spawn(scene, "Bumper Mine", position, yaw);
    cube(
        scene,
        mine,
        "Base",
        vec3(0.0, 0.08, 0.0),
        vec3(1.5, 0.16, 1.5),
        BLACK_MATERIAL,
    );
    cube(
        scene,
        mine,
        "Spring",
        vec3(0.0, 0.22, 0.0),
        vec3(0.6, 0.18, 0.6),
        METAL_MATERIAL,
    );
    cube(
        scene,
        mine,
        "Pad",
        vec3(0.0, 0.36, 0.0),
        vec3(1.2, 0.12, 1.2),
        YELLOW_MATERIAL,
    );
    scene.add_component(
        mine,
        ComponentBumperMine {
            owner: GameObjectRef::new(owner),
            arm_time: abilities.mine_arm_time,
            radius: abilities.mine_radius,
            lift: abilities.mine_lift,
            push: abilities.mine_push,
            growth: abilities.mine_growth,
            damage: abilities.mine_damage,
            age: 0.0,
        },
    );
}

/// Lays an oil slick behind `tank`.
fn lay_oil(scene: &mut Scene, tank: GameObject, abilities: &ComponentTankAbilities) {
    let pose = scene.world_transform(tank);
    let back = flatten_xz(pose.rotation * Vec3::z());
    let yaw =
        UnitQuaternion::from_euler_angles(0.0, crate::tank::rotation_yaw(&pose.rotation), 0.0);
    let mut position = pose.position - back * (abilities.oil_half_size + 1.0);
    position.y = super::ground_height(scene, position).unwrap_or(pose.position.y);
    let oil = spawn(scene, "Oil Slick", position, yaw);
    let side = abilities.oil_half_size * 2.0;
    cube(
        scene,
        oil,
        "Slick",
        vec3(0.0, 0.015, 0.0),
        vec3(side, 0.03, side),
        BLACK_MATERIAL,
    );
    scene.add_component(
        oil,
        ComponentOilSlick {
            half_size: abilities.oil_half_size,
            lifetime: abilities.oil_lifetime,
            traction: abilities.oil_traction,
            tumble_friction: abilities.oil_tumble_friction,
            age: 0.0,
        },
    );
}

/// PROTOTYPE (#610). A bumper mine: the next tank to drive over it is
/// launched up and out of control, harder the higher its meter.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "abdeae08-f8db-4314-9e69-55fdc8c90c44"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Bumper Mine (prototype)")]
#[serde(default)]
pub struct ComponentBumperMine {
    pub owner: GameObjectRef,
    /// Seconds before it can go off.
    pub arm_time: f32,
    /// How close a tank must come to set it off, in metres.
    pub radius: f32,
    /// Upward launch at 0%, in metres per second.
    pub lift: f32,
    /// Outward launch at 0%, in metres per second.
    pub push: f32,
    /// How much its launch grows per percent on the meter.
    pub growth: f32,
    /// Percent it adds to the meter.
    pub damage: f32,
    /// Seconds since it was dropped.
    pub age: f32,
}

impl Default for ComponentBumperMine {
    fn default() -> Self {
        Self {
            owner: Default::default(),
            arm_time: 0.6,
            radius: 1.5,
            lift: 10.0,
            push: 5.0,
            growth: 0.01,
            damage: 8.0,
            age: 0.0,
        }
    }
}

impl Component for ComponentBumperMine {}

impl ComponentUpdate for ComponentBumperMine {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let dt = resources.time().delta_time().max(0.0);
        let Some(mine) = this.write(scene, |mine| {
            mine.age += dt;
            *mine
        }) else {
            return;
        };
        if mine.age < mine.arm_time {
            return;
        }
        let center = scene.world_transform(game_object).position;
        let victim = scene.objects_with::<ComponentKnockout>().find(|&tank| {
            let position = scene.world_transform(tank).position;
            on_patch(center, mine.radius, position)
                && (position.y - center.y).abs() < 1.5
                && scene
                    .read_component::<ComponentKnockout, _, _>(tank, |knockout| {
                        knockout.state.is_upright()
                    })
                    .unwrap_or(false)
        });
        let Some(victim) = victim else {
            return;
        };
        let away = scene.world_transform(victim).position - center;
        let velocity = bumper_launch(mine.lift, mine.push, 0.0, 0.0, away);
        launch(scene, victim, velocity, mine.growth, mine.damage);
        for offset in [Vec3::zeros(), vec3(0.4, 0.3, 0.0), vec3(-0.4, 0.2, 0.3)] {
            crate::combat_vfx::spawn_impact(scene, center + offset + Vec3::y() * 0.4, true);
        }
        scene.delete(game_object);
    }
}

/// PROTOTYPE (#610). An oil slick: tanks on it lose most of their grip, and
/// tumbling tanks slide much further.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "0a2d2fe3-ed44-446a-a953-4c238ed18e3a"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Oil Slick (prototype)")]
#[serde(default)]
pub struct ComponentOilSlick {
    /// Half its side, in metres.
    pub half_size: f32,
    /// Seconds it lasts.
    pub lifetime: f32,
    /// Share of their grip tanks keep on it.
    pub traction: f32,
    /// Share of their friction tumbling tanks keep on it.
    pub tumble_friction: f32,
    /// Seconds since it was laid.
    pub age: f32,
}

impl Default for ComponentOilSlick {
    fn default() -> Self {
        Self {
            half_size: 2.75,
            lifetime: 8.0,
            traction: 0.15,
            tumble_friction: 0.05,
            age: 0.0,
        }
    }
}

impl Component for ComponentOilSlick {}

impl ComponentUpdate for ComponentOilSlick {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let dt = resources.time().delta_time().max(0.0);
        let expired = this
            .write(scene, |oil| {
                oil.age += dt;
                oil.age >= oil.lifetime
            })
            .unwrap_or(false);
        if expired {
            scene.delete(game_object);
        }
    }
}

/// The slipperiest oil slick under `position`, if any.
pub(crate) fn oil_under(scene: &Scene, position: Vec3) -> Option<ComponentOilSlick> {
    scene
        .objects_with::<ComponentOilSlick>()
        .filter_map(|oil| {
            let center = scene.world_transform(oil).position;
            scene
                .read_component::<ComponentOilSlick, _, _>(oil, |slick| {
                    (on_patch(center, slick.half_size, position)
                        && (position.y - center.y).abs() < 2.0)
                        .then_some(*slick)
                })
                .flatten()
        })
        .min_by(|a, b| a.traction.total_cmp(&b.traction))
}
