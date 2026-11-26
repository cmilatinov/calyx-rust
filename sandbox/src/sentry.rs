use crate::spawn::ComponentRespawnState;
use crate::tank::{flatten_xz, shot_direction, spawn_round, yaw_rotation, ComponentProjectile};
use crate::terrain::{drive_surfaces, surface_height_below};
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
use serde::{Deserialize, Serialize};

/// A fixed turret that turns to its target and fires rounds at it while the
/// target is alive and in range. Cover absorbs its rounds like the tank's.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "3e9d6c1a-7b52-4f08-a4c3-95e1d2b7f604"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Sentry")]
#[serde(default)]
pub struct ComponentSentry {
    pub target: GameObjectRef,
    /// The visual head to turn. An unassigned head keeps legacy root turning.
    pub head: GameObjectRef,
    pub range: f32,
    pub fire_interval: f32,
    pub projectile_speed: f32,
    pub projectile_damage: f32,
    pub projectile_lifetime: f32,
    pub projectile_radius: f32,
    pub muzzle_offset: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub fire_cooldown_remaining: f32,
}

impl Default for ComponentSentry {
    fn default() -> Self {
        Self {
            target: Default::default(),
            head: Default::default(),
            range: 16.0,
            fire_interval: 1.5,
            projectile_speed: 16.0,
            projectile_damage: 25.0,
            projectile_lifetime: 1.5,
            projectile_radius: 0.2,
            muzzle_offset: 1.2,
            fire_cooldown_remaining: 0.0,
        }
    }
}

impl Component for ComponentSentry {}

impl ComponentUpdate for ComponentSentry {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_sentry(scene, this.game_object(), resources.time().delta_time());
    }
}

fn update_sentry(scene: &mut Scene, game_object: GameObject, dt: f32) {
    if crate::game_state::combat_paused(scene) {
        return;
    }
    let Some(mut sentry) =
        scene.read_component::<ComponentSentry, _, _>(game_object, |sentry| *sentry)
    else {
        return;
    };
    sentry.fire_cooldown_remaining = (sentry.fire_cooldown_remaining - dt.max(0.0)).max(0.0);
    fire_at_target(scene, game_object, &mut sentry);
    let _ = scene.write_component::<ComponentSentry, _>(game_object, |component| {
        component.fire_cooldown_remaining = sentry.fire_cooldown_remaining;
    });
}

fn fire_at_target(scene: &mut Scene, game_object: GameObject, sentry: &mut ComponentSentry) {
    let Some(target) = sentry.target.game_object(scene) else {
        return;
    };
    let target_locked = scene
        .read_component::<ComponentRespawnState, _, _>(target, |state| state.is_gameplay_locked())
        .unwrap_or(false);
    if target_locked {
        return;
    }

    let mut transform = scene.world_transform(game_object);
    let offset = scene.world_transform(target).position - transform.position;
    let distance = vec_xz_length(offset);
    if distance <= f32::EPSILON || distance > sentry.range {
        return;
    }
    // Brush hides a tank from a sentry outside its patch, as from a tank.
    if crate::brush::hides_from(scene, transform.position, target) {
        return;
    }

    let direction = flatten_xz(offset);
    transform.rotation = yaw_rotation(&direction);
    let head = sentry.head.game_object(scene).unwrap_or(game_object);
    let mut head_transform = scene.world_transform(head);
    head_transform.rotation = transform.rotation;
    scene.set_world_transform(head, head_transform.matrix());
    if sentry.fire_cooldown_remaining > f32::EPSILON {
        return;
    }

    // Like the tank's, its round flies at the gun's height above the ground
    // under the target, climbing to a tank on a raised deck.
    let muzzle = transform.position + direction * sentry.muzzle_offset;
    let target_ground = scene.world_transform(target).position;
    let ground = surface_height_below(
        scene,
        &drive_surfaces(scene),
        transform.position,
        SENTRY_GROUND_REACH,
    )
    .unwrap_or(target_ground.y);
    let flight = shot_direction(muzzle, direction, muzzle.y - ground, Some(target_ground));

    spawn_round(
        scene,
        muzzle,
        UnitQuaternion::rotation_between(&direction, &flight)
            .unwrap_or_else(UnitQuaternion::identity)
            * transform.rotation,
        ComponentProjectile {
            direction: flight,
            speed: sentry.projectile_speed,
            damage: sentry.projectile_damage,
            lifetime_remaining: sentry.projectile_lifetime,
            radius: sentry.projectile_radius,
            shooter: GameObjectRef::new(scene.uuid(game_object)),
        },
    );
    sentry.fire_cooldown_remaining = sentry.fire_interval;
}

/// How far below its root the sentry looks for the ground it stands on.
const SENTRY_GROUND_REACH: f32 = 2.0;

fn vec_xz_length(vector: nalgebra_glm::Vec3) -> f32 {
    (vector.x * vector.x + vector.z * vector.z).sqrt()
}

#[cfg(test)]
mod tests;
