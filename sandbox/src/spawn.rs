use crate::tank::ComponentHealth;
use engine::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use engine::input::Input;
use engine::math::Transform;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::{GameObject, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

pub const SPAWN_TEAM_ANY: u32 = u32::MAX;

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "876c7327-0d92-4242-b567-fd9a258c6c06"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Spawn Point")]
#[serde(default)]
#[repr(C)]
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
#[repr(C)]
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
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_respawn_state(scene, game_object, resources.time().delta_time());
    }
}

pub fn update_respawn_state(scene: &mut Scene, game_object: GameObject, dt: f32) -> bool {
    let Some(mut state) =
        scene.read_component::<ComponentRespawnState, _, _>(game_object, |component| *component)
    else {
        return false;
    };

    let should_spawn = state.tick(dt) == Some(RespawnAction::Spawn);
    let spawned = should_spawn
        && find_spawn_transform(scene, state.team).is_some_and(|mut transform| {
            let respawn_target = respawn_transform_target(scene, game_object);
            transform.scale = scene.world_transform(respawn_target).scale;
            scene.set_world_transform(respawn_target, transform.matrix());
            state.mark_spawned();
            true
        });

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

fn respawn_transform_target(scene: &Scene, game_object: GameObject) -> GameObject {
    scene
        .ancestors(game_object)
        .take_while(|parent| *parent != scene.root())
        .last()
        .unwrap_or(game_object)
}

pub fn find_spawn_transform(scene: &Scene, team: u32) -> Option<Transform> {
    scene
        .objects()
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
mod tests {
    use super::{
        find_spawn_transform, update_respawn_state, ComponentRespawnState, ComponentSpawnPoint,
        RespawnAction, SPAWN_TEAM_ANY,
    };
    use engine::math::Transform;
    use nalgebra_glm::vec3;

    #[test]
    fn death_request_starts_respawn_timer() {
        let mut state = ComponentRespawnState {
            respawn_delay: 4.0,
            invulnerability_remaining: 1.0,
            death_requested: true,
            ..Default::default()
        };

        assert_eq!(state.tick(0.5), None);
        assert!(!state.alive);
        assert_eq!(state.respawn_remaining, 4.0);
        assert_eq!(state.invulnerability_remaining, 0.0);
        assert!(!state.death_requested);
    }

    #[test]
    fn dead_state_requests_spawn_when_timer_expires() {
        let mut state = ComponentRespawnState {
            alive: false,
            respawn_remaining: 0.25,
            ..Default::default()
        };

        assert_eq!(state.tick(0.5), Some(RespawnAction::Spawn));
        assert!(state.respawn_requested);
    }

    #[test]
    fn live_respawn_request_does_not_bypass_next_death_delay() {
        let mut state = ComponentRespawnState {
            respawn_delay: 2.0,
            respawn_requested: true,
            ..Default::default()
        };

        assert_eq!(state.tick(0.0), None);
        assert!(state.alive);
        assert!(!state.respawn_requested);

        state.respawn_requested = true;
        state.death_requested = true;
        assert_eq!(state.tick(0.0), None);
        assert!(!state.alive);
        assert_eq!(state.respawn_remaining, 2.0);
        assert!(!state.respawn_requested);

        assert_eq!(state.tick(0.5), None);
        assert_eq!(state.respawn_remaining, 1.5);
    }

    #[test]
    fn spawned_state_restores_alive_with_invulnerability() {
        let mut state = ComponentRespawnState {
            alive: false,
            respawn_requested: true,
            invulnerability_duration: 1.5,
            ..Default::default()
        };

        state.mark_spawned();

        assert!(state.alive);
        assert!(!state.respawn_requested);
        assert!(state.is_invulnerable());
        assert_eq!(state.invulnerability_remaining, 1.5);
    }

    #[test]
    fn protected_respawn_state_locks_gameplay() {
        assert!(ComponentRespawnState {
            alive: false,
            ..Default::default()
        }
        .is_gameplay_locked());
        assert!(ComponentRespawnState {
            death_requested: true,
            ..Default::default()
        }
        .is_gameplay_locked());
        assert!(ComponentRespawnState {
            invulnerability_remaining: 1.0,
            ..Default::default()
        }
        .is_gameplay_locked());
        assert!(!ComponentRespawnState::default().is_gameplay_locked());
    }

    #[test]
    fn spawn_lookup_prefers_matching_team_then_order() {
        let mut scene = engine::test_support::test_scene();
        let generic = scene.create(None, None);
        let team_late = scene.create(None, None);
        let team_first = scene.create(None, None);
        scene.set_world_transform(generic, Transform::from_xyz(1.0, 0.0, 0.0).matrix());
        scene.set_world_transform(team_late, Transform::from_xyz(2.0, 0.0, 0.0).matrix());
        scene.set_world_transform(team_first, Transform::from_xyz(3.0, 0.0, 0.0).matrix());
        scene.add_component(
            generic,
            ComponentSpawnPoint {
                team: SPAWN_TEAM_ANY,
                order: 0,
                ..Default::default()
            },
        );
        scene.add_component(
            team_late,
            ComponentSpawnPoint {
                team: 2,
                order: 10,
                ..Default::default()
            },
        );
        scene.add_component(
            team_first,
            ComponentSpawnPoint {
                team: 2,
                order: 2,
                ..Default::default()
            },
        );

        let transform = find_spawn_transform(&scene, 2).expect("team spawn should resolve");

        assert_eq!(transform.position, vec3(3.0, 0.0, 0.0));
    }

    #[test]
    fn respawn_update_moves_object_to_spawn_point() {
        let mut scene = engine::test_support::test_scene();
        let spawn = scene.create(None, None);
        let player = scene.create(None, None);
        scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
        scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
        scene.add_component(
            spawn,
            ComponentSpawnPoint {
                team: 3,
                ..Default::default()
            },
        );
        scene.add_component(
            player,
            ComponentRespawnState {
                team: 3,
                alive: false,
                respawn_remaining: 0.0,
                invulnerability_duration: 2.5,
                ..Default::default()
            },
        );

        assert!(update_respawn_state(&mut scene, player, 0.0));

        let player_transform = scene.world_transform(player);
        assert_eq!(player_transform.position, vec3(8.0, 0.0, 4.0));
        let state = scene
            .read_component::<ComponentRespawnState, _, _>(player, |state| *state)
            .expect("player should keep respawn state");
        assert!(state.alive);
        assert_eq!(state.invulnerability_remaining, 2.5);
    }

    #[test]
    fn respawn_update_preserves_object_scale() {
        let mut scene = engine::test_support::test_scene();
        let spawn = scene.create(None, None);
        let player = scene.create(None, None);
        let mut spawn_transform = Transform::from_xyz(8.0, 0.0, 4.0);
        spawn_transform.scale = vec3(5.0, 5.0, 5.0);
        let mut player_transform = Transform::from_xyz(-4.0, 0.0, 0.0);
        player_transform.scale = vec3(0.5, 1.5, 2.0);
        scene.set_world_transform(spawn, spawn_transform.matrix());
        scene.set_world_transform(player, player_transform.matrix());
        scene.add_component(
            spawn,
            ComponentSpawnPoint {
                team: 3,
                ..Default::default()
            },
        );
        scene.add_component(
            player,
            ComponentRespawnState {
                team: 3,
                alive: false,
                respawn_remaining: 0.0,
                ..Default::default()
            },
        );

        assert!(update_respawn_state(&mut scene, player, 0.0));

        let updated = scene.world_transform(player);
        assert_eq!(updated.position, vec3(8.0, 0.0, 4.0));
        assert!((updated.scale - vec3(0.5, 1.5, 2.0)).magnitude() < 1e-6);
    }

    #[test]
    fn respawn_update_moves_the_complete_player_rig() {
        let mut scene = engine::test_support::test_scene();
        let spawn = scene.create(None, None);
        let player = scene.create(None, None);
        let tank = scene.create(None, None);
        let camera = scene.create(None, None);
        let crosshair = scene.create(None, None);
        scene.set_parent(tank, Some(player));
        scene.set_parent(camera, Some(player));
        scene.set_parent(crosshair, Some(player));
        scene.set_transform(tank, &Transform::from_xyz(2.0, 0.0, 0.0).matrix());
        scene.set_transform(camera, &Transform::from_xyz(0.0, 16.0, -10.0).matrix());
        scene.set_transform(crosshair, &Transform::from_xyz(0.0, 0.0, 6.0).matrix());
        scene.set_world_transform(spawn, Transform::from_xyz(8.0, 0.0, 4.0).matrix());
        scene.set_world_transform(player, Transform::from_xyz(-4.0, 0.0, 0.0).matrix());
        scene.add_component(spawn, ComponentSpawnPoint::default());
        scene.add_component(
            tank,
            ComponentRespawnState {
                alive: false,
                respawn_remaining: 0.0,
                ..Default::default()
            },
        );

        assert!(update_respawn_state(&mut scene, tank, 0.0));

        assert_eq!(scene.world_transform(player).position, vec3(8.0, 0.0, 4.0));
        assert_eq!(scene.world_transform(tank).position, vec3(10.0, 0.0, 4.0));
        assert_eq!(
            scene.world_transform(camera).position,
            vec3(8.0, 16.0, -6.0)
        );
        assert_eq!(
            scene.world_transform(crosshair).position,
            vec3(8.0, 0.0, 10.0)
        );
    }
}
