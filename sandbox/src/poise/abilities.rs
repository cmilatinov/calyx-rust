//! PROTOTYPE (poise_poc, #610): the player's boot and grappling hook.

use super::logic::{Stance, Swing, SwingTuning};
use super::{
    hull_blocked, knock_down, line_clear, place, strike, Blow, ComponentDummyDriver, ComponentPoise,
};
use crate::tank::{flatten_xz, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};

/// The input action that fires the boot.
pub const BOOT_ACTION: &str = "boot";
/// The input action that fires, holds and releases the hook.
pub const HOOK_ACTION: &str = "hook";

/// How high above a tank's origin the hook line attaches, in metres.
const HOOK_HEIGHT: f32 = 1.0;
/// How close the flying hook must come to its target to latch, in metres.
const LATCH_DISTANCE: f32 = 0.8;

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
    /// Latched on and swinging `target` round.
    Latched { target: GameObjectRef, swing: Swing },
    /// Coming back empty.
    Retracting { head: Vec3 },
}

/// PROTOTYPE (#610). The boot and the grappling hook, with their tuning.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "5f74cef2-5b9d-46bd-922a-3b5a6f111a30"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Boot and Hook (prototype)")]
#[serde(default)]
pub struct ComponentTankAbilities {
    /// How far in front of the tank's centre the boot reaches, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub boot_range: f32,
    /// Half the angle in front of the tank the boot covers, in degrees.
    #[reflect_attr(min = 0.0, max = 180.0, speed = 0.5)]
    pub boot_half_angle: f32,
    /// Poise the boot takes.
    #[reflect_attr(min = 0.0, speed = 1.0)]
    pub boot_poise_damage: f32,
    /// Shove speed of a boot that does not break poise, in metres per
    /// second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_shove: f32,
    /// Launch speed of a boot that breaks poise, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_launch_speed: f32,
    /// Upward speed of a breaking boot, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_lift: f32,
    /// Spin of a breaking boot, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_spin: f32,
    /// How fast the booting tank lunges forward, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub boot_lunge: f32,
    /// Seconds between boots.
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
    /// The radius a hooked tank swings round at, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_radius: f32,
    /// How fast the hook reels a tank in or out to the swing radius, in
    /// metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub reel_speed: f32,
    /// How fast the swing speeds up, in radians per second squared.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub swing_acceleration: f32,
    /// The swing's top speed, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_speed: f32,
    /// Swing clockwise seen from above, instead of anticlockwise.
    pub swing_clockwise: bool,
    /// How high a hooked tank is lifted while it swings, in metres.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub swing_height: f32,
    /// The throw's speed as a share of the swing's speed at release.
    #[reflect_attr(min = 0.0, speed = 0.05)]
    pub throw_strength: f32,
    /// Upward speed of a throw, in metres per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub throw_lift: f32,
    /// Spin of a throw, in radians per second.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub throw_spin: f32,

    #[serde(skip)]
    #[reflect_skip]
    pub(crate) boot_cooldown_remaining: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook_cooldown_remaining: f32,
    /// Seconds since the last boot, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) since_boot: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook: Hook,
    /// Where the hook line leaves the tank, for the HUD.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) hook_origin: Vec3,
}

impl Default for ComponentTankAbilities {
    fn default() -> Self {
        Self {
            boot_range: 4.2,
            boot_half_angle: 55.0,
            boot_poise_damage: 60.0,
            boot_shove: 14.0,
            boot_launch_speed: 13.0,
            boot_lift: 6.5,
            boot_spin: 7.0,
            boot_lunge: 6.0,
            boot_cooldown: 1.2,
            hook_range: 18.0,
            hook_aim_assist: 5.0,
            hook_speed: 70.0,
            hook_cooldown: 3.0,
            hook_miss_cooldown: 0.8,
            swing_radius: 5.5,
            reel_speed: 12.0,
            swing_acceleration: 7.0,
            swing_speed: 3.6,
            swing_clockwise: false,
            swing_height: 1.3,
            throw_strength: 1.0,
            throw_lift: 5.0,
            throw_spin: 8.0,
            boot_cooldown_remaining: 0.0,
            hook_cooldown_remaining: 0.0,
            since_boot: f32::INFINITY,
            hook: Hook::Idle,
            hook_origin: Vec3::zeros(),
        }
    }
}

impl Component for ComponentTankAbilities {}

impl ComponentTankAbilities {
    fn swing_tuning(&self) -> SwingTuning {
        SwingTuning {
            radius: self.swing_radius,
            reel_speed: self.reel_speed,
            acceleration: self.swing_acceleration,
            max_speed: self.swing_speed,
            direction: if self.swing_clockwise { -1.0 } else { 1.0 },
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
        if crate::game_state::combat_paused(scene)
            || scene
                .read_component::<ComponentDummyDriver, _, _>(game_object, |_| ())
                .is_some()
        {
            return;
        }
        let dt = resources.time().delta_time().max(0.0);
        let Some(mut abilities) = this.read(scene, |abilities| *abilities) else {
            return;
        };
        abilities.boot_cooldown_remaining = (abilities.boot_cooldown_remaining - dt).max(0.0);
        abilities.hook_cooldown_remaining = (abilities.hook_cooldown_remaining - dt).max(0.0);
        abilities.since_boot += dt;
        abilities.hook_origin = hook_origin(scene, game_object);

        let upright = scene
            .read_component::<ComponentPoise, _, _>(game_object, |poise| poise.state.is_upright())
            .unwrap_or(true);
        if !upright {
            // Knocked down: no abilities, and the hook lets go.
            release_hook(scene, &mut abilities);
            let _ = this.write(scene, |component| *component = abilities);
            return;
        }

        if input.action(BOOT_ACTION).just_pressed() && abilities.boot_cooldown_remaining <= 0.0 {
            boot(scene, game_object, &mut abilities);
        }
        update_hook(
            scene,
            game_object,
            &mut abilities,
            input.action(HOOK_ACTION).pressed(),
            input.action(HOOK_ACTION).just_pressed(),
            dt,
        );
        let _ = this.write(scene, |component| *component = abilities);
    }
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

/// Kicks every tank with poise in front of `tank`, within the boot's reach.
fn boot(scene: &mut Scene, tank: GameObject, abilities: &mut ComponentTankAbilities) {
    abilities.boot_cooldown_remaining = abilities.boot_cooldown;
    abilities.since_boot = 0.0;
    let pose = scene.world_transform(tank);
    let forward = flatten_xz(pose.rotation * Vec3::z());
    let cos_half = abilities.boot_half_angle.to_radians().cos();
    let targets: Vec<_> = scene
        .objects_with::<ComponentPoise>()
        .filter(|&other| other != tank)
        .filter(|&other| {
            let offset = scene.world_transform(other).position - pose.position;
            let flat = flatten_xz(offset);
            offset.xz().magnitude() <= abilities.boot_range
                && (flat == Vec3::zeros() || flat.dot(&forward) >= cos_half)
        })
        .collect();
    for target in &targets {
        let offset = flatten_xz(scene.world_transform(*target).position - pose.position);
        let direction = flatten_xz(forward * 1.5 + offset);
        strike(
            scene,
            *target,
            Blow {
                poise_damage: abilities.boot_poise_damage,
                direction,
                shove: abilities.boot_shove,
                launch: abilities.boot_launch_speed,
                lift: abilities.boot_lift,
                spin: abilities.boot_spin,
                juggle: abilities.boot_launch_speed * 0.6,
            },
        );
        let at = scene.world_transform(*target).position + Vec3::y() * 0.8;
        crate::combat_vfx::spawn_impact(scene, at, true);
    }
    // The booting tank lunges into the kick.
    let _ = scene.write_component::<ComponentPoise, _>(tank, |poise| {
        poise.knockback += forward * abilities.boot_lunge;
    });
    crate::combat_vfx::spawn_impact(
        scene,
        pose.position + forward * 2.0 + Vec3::y() * 0.3,
        false,
    );
}

/// The tank with poise nearest `aim` within the hook's aim assist and range
/// of `tank`, that the hook could latch onto now.
fn hook_target(
    scene: &Scene,
    tank: GameObject,
    origin: Vec3,
    aim: Vec3,
    abilities: &ComponentTankAbilities,
) -> Option<GameObject> {
    let center = scene.world_transform(tank).position;
    scene
        .objects_with::<ComponentPoise>()
        .filter(|&other| other != tank)
        .filter_map(|other| {
            let position = scene.world_transform(other).position;
            let hookable = scene
                .read_component::<ComponentPoise, _, _>(other, |poise| {
                    !poise.state.is_immune() && poise.state.stance != Stance::Hooked
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

/// The point the player aims at: the tank's crosshair, or straight ahead.
fn aim_point(scene: &Scene, tank: GameObject, range: f32) -> Vec3 {
    let pose = scene.world_transform(tank);
    scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| controller.crosshair)
        .and_then(|crosshair| crosshair.game_object(scene))
        .map(|crosshair| scene.world_transform(crosshair).position)
        .unwrap_or_else(|| pose.position + flatten_xz(pose.rotation * Vec3::z()) * range)
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
        Hook::Latched { target, mut swing } => {
            let Some(target_object) = target.game_object(scene) else {
                abilities.hook = Hook::Idle;
                return;
            };
            let still_hooked = scene
                .read_component::<ComponentPoise, _, _>(target_object, |poise| {
                    poise.state.stance == Stance::Hooked
                })
                .unwrap_or(false);
            if !still_hooked {
                abilities.hook = Hook::Idle;
                return;
            }
            if !held {
                // Let go: thrown along the tangent, as hard as the swing.
                let velocity = swing.throw_velocity(abilities.throw_strength, abilities.throw_lift);
                knock_down(scene, target_object, velocity, abilities.throw_spin);
                abilities.hook = Hook::Idle;
                abilities.hook_cooldown_remaining = abilities.hook_cooldown;
                return;
            }
            let from = scene.world_transform(target_object);
            swing.advance(dt, &abilities.swing_tuning());
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
            abilities.hook = Hook::Latched { target, swing };
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
    let Some(poise) = scene.write_component::<ComponentPoise, _>(target_object, |poise| {
        if poise.state.is_upright() {
            poise.rest_height = height;
        }
        poise.state.stance = Stance::Hooked;
        poise.knockback = Vec3::zeros();
        poise.clone()
    }) else {
        abilities.hook = Hook::Idle;
        return;
    };
    super::set_body_type(
        scene,
        target_object,
        rapier3d::prelude::RigidBodyType::KinematicPositionBased,
        &poise,
    );
    let center = scene.world_transform(tank).position;
    let position = scene.world_transform(target_object).position;
    abilities.hook = Hook::Latched {
        target,
        swing: Swing::starting_at(center, position),
    };
    crate::combat_vfx::spawn_impact(scene, position + Vec3::y() * HOOK_HEIGHT, true);
}

/// Lets go of whatever the hook holds: a held tank drops, tumbling, as it
/// was going.
fn release_hook(scene: &mut Scene, abilities: &mut ComponentTankAbilities) {
    match abilities.hook {
        Hook::Latched { target, swing } => {
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
