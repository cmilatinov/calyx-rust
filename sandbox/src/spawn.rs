use crate::tank::ComponentHealth;
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

pub const SPAWN_TEAM_ANY: u32 = u32::MAX;

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "876c7327-0d92-4242-b567-fd9a258c6c06"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Spawn Point")]
#[serde(default)]
pub struct ComponentSpawnPoint {
    pub team: u32,
    pub order: u32,
    pub radius: f32,
    pub active: bool,
}

impl Default for ComponentSpawnPoint {
    fn default() -> Self {
        Self {
            team: SPAWN_TEAM_ANY,
            order: 0,
            radius: 1.0,
            active: true,
        }
    }
}

impl Component for ComponentSpawnPoint {}

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "28ca6ba9-20bd-4077-8f35-1cf8483c6f4e"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Respawn State")]
#[serde(default)]
pub struct ComponentRespawnState {
    pub team: u32,
    pub respawn_delay: f32,
    pub respawn_remaining: f32,
    pub invulnerability_duration: f32,
    pub invulnerability_remaining: f32,
    pub alive: bool,
    pub death_requested: bool,
    pub respawn_requested: bool,
}

impl Default for ComponentRespawnState {
    fn default() -> Self {
        Self {
            team: SPAWN_TEAM_ANY,
            respawn_delay: 3.0,
            respawn_remaining: 0.0,
            invulnerability_duration: 2.0,
            invulnerability_remaining: 0.0,
            alive: true,
            death_requested: false,
            respawn_requested: false,
        }
    }
}

impl Component for ComponentRespawnState {}

impl ComponentRespawnState {
    pub fn is_invulnerable(&self) -> bool {
        self.alive && self.invulnerability_remaining > f32::EPSILON
    }

    /// Returns whether gameplay should be disabled during death, respawn, or
    /// post-spawn invulnerability.
    pub fn is_gameplay_locked(&self) -> bool {
        !self.alive || self.death_requested || self.is_invulnerable()
    }

    pub fn request_death(&mut self) {
        self.death_requested = true;
    }

    pub fn request_respawn(&mut self) {
        self.respawn_requested = true;
    }

    fn tick(&mut self, dt: f32) -> Option<RespawnAction> {
        let dt = dt.max(0.0);

        if self.death_requested {
            self.death_requested = false;
            if self.alive {
                self.alive = false;
                self.respawn_requested = false;
                self.respawn_remaining = self.respawn_delay.max(0.0);
                self.invulnerability_remaining = 0.0;
            }
            return None;
        }

        if self.alive {
            self.respawn_requested = false;
            self.invulnerability_remaining = (self.invulnerability_remaining - dt).max(0.0);
            return None;
        }

        self.respawn_remaining = (self.respawn_remaining - dt).max(0.0);
        if self.respawn_remaining <= f32::EPSILON {
            self.respawn_requested = true;
        }

        self.respawn_requested.then_some(RespawnAction::Spawn)
    }

    fn mark_spawned(&mut self) {
        self.alive = true;
        self.death_requested = false;
        self.respawn_requested = false;
        self.respawn_remaining = 0.0;
        self.invulnerability_remaining = self.invulnerability_duration.max(0.0);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RespawnAction {
    Spawn,
}

impl ComponentUpdate for ComponentRespawnState {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_respawn_state(scene, this.game_object(), resources.time().delta_time());
    }
}

pub fn update_respawn_state(scene: &mut Scene, game_object: GameObject, dt: f32) -> bool {
    // A tank destroyed as the match ended stays down on the final scoreboard,
    // and nobody comes back outside play.
    if crate::game_state::combat_paused(scene) {
        return false;
    }
    let Some(mut state) =
        scene.read_component::<ComponentRespawnState, _, _>(game_object, |component| *component)
    else {
        return false;
    };

    let should_spawn = state.tick(dt) == Some(RespawnAction::Spawn);
    let spawned = should_spawn && place_at_spawn(scene, game_object, state.team);
    if spawned {
        state.mark_spawned();
    }

    if spawned {
        let _ = scene.write_component::<ComponentHealth, _>(game_object, |health| {
            health.restore_full();
        });
    }

    let _ = scene.write_component::<ComponentRespawnState, _>(game_object, |component| {
        *component = state;
    });
    spawned
}

/// Moves `game_object`, with the rig it belongs to, onto `team`'s spawn
/// point, facing the way the point faces. Returns `false`, leaving it where
/// it is, when the scene has no spawn point for `team`.
fn place_at_spawn(scene: &mut Scene, game_object: GameObject, team: u32) -> bool {
    let Some(transform) = find_spawn_transform(scene, team) else {
        return false;
    };
    let respawn_target = respawn_transform_target(scene, game_object);
    let mut target_transform = scene.world_transform(respawn_target);
    target_transform.position += transform.position - scene.world_transform(game_object).position;
    scene.set_world_transform(respawn_target, target_transform.matrix());
    let mut controlled_transform = scene.world_transform(game_object);
    controlled_transform.rotation = transform.rotation;
    scene.set_world_transform(game_object, controlled_transform.matrix());
    true
}

/// Starts `game_object` afresh for a new match: alive at its spawn point, with
/// no respawn pending and no spawn protection, so it can move as soon as the
/// match starts. Its health is restored separately.
pub(crate) fn reset_for_match(scene: &mut Scene, game_object: GameObject) {
    let Some(team) =
        scene.read_component::<ComponentRespawnState, _, _>(game_object, |state| state.team)
    else {
        return;
    };
    place_at_spawn(scene, game_object, team);
    let _ = scene.write_component::<ComponentRespawnState, _>(game_object, |state| {
        state.alive = true;
        state.death_requested = false;
        state.respawn_requested = false;
        state.respawn_remaining = 0.0;
        state.invulnerability_remaining = 0.0;
    });
}

fn respawn_transform_target(scene: &Scene, game_object: GameObject) -> GameObject {
    scene
        .ancestors(game_object)
        .find(|parent| *parent != scene.root())
        .unwrap_or(game_object)
}

pub fn find_spawn_transform(scene: &Scene, team: u32) -> Option<Transform> {
    scene
        .objects_with::<ComponentSpawnPoint>()
        .filter_map(|game_object| {
            scene
                .read_component::<ComponentSpawnPoint, _, _>(game_object, |spawn| *spawn)
                .filter(|spawn| spawn.active)
                .filter(|spawn| spawn.team == team || spawn.team == SPAWN_TEAM_ANY)
                .map(|spawn| {
                    let team_rank = u32::from(spawn.team != team);
                    ((team_rank, spawn.order), scene.world_transform(game_object))
                })
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, transform)| transform)
}

#[cfg(test)]
mod tests;
