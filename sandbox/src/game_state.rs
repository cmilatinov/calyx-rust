use crate::tank::{ComponentHealth, ComponentProjectile, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use legion::IntoQuery;
use serde::{Deserialize, Serialize};

pub const GAME_PHASE_MAIN_MENU: u32 = 0;
pub const GAME_PHASE_LOBBY: u32 = 1;
pub const GAME_PHASE_PLAYING: u32 = 2;
pub const GAME_PHASE_GAME_OVER: u32 = 3;
pub const GAME_PHASE_INVALID: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum GamePhase {
    MainMenu = GAME_PHASE_MAIN_MENU,
    Lobby = GAME_PHASE_LOBBY,
    Playing = GAME_PHASE_PLAYING,
    GameOver = GAME_PHASE_GAME_OVER,
}

impl GamePhase {
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub const fn from_u32(value: u32) -> Self {
        match value {
            GAME_PHASE_LOBBY => Self::Lobby,
            GAME_PHASE_PLAYING => Self::Playing,
            GAME_PHASE_GAME_OVER => Self::GameOver,
            _ => Self::MainMenu,
        }
    }
}

#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "58a0bdbb-d356-4192-a473-b84314830748"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Game State")]
#[serde(default)]
pub struct ComponentGameState {
    pub phase: u32,
    pub previous_phase: u32,
    pub phase_elapsed: f32,
    pub match_duration: f32,
    pub match_time_remaining: f32,
    pub winning_team: u32,
    pub enter_lobby_requested: bool,
    pub start_match_requested: bool,
    pub players_ready: bool,
    pub finish_match_requested: bool,
    pub restart_match_requested: bool,
    pub return_to_menu_requested: bool,
}

impl Default for ComponentGameState {
    fn default() -> Self {
        Self {
            phase: GamePhase::MainMenu.as_u32(),
            previous_phase: GAME_PHASE_INVALID,
            phase_elapsed: 0.0,
            match_duration: 300.0,
            match_time_remaining: 300.0,
            winning_team: GAME_PHASE_INVALID,
            enter_lobby_requested: false,
            start_match_requested: false,
            players_ready: false,
            finish_match_requested: false,
            restart_match_requested: false,
            return_to_menu_requested: false,
        }
    }
}

impl Component for ComponentGameState {}

impl ComponentGameState {
    pub fn current_phase(&self) -> GamePhase {
        GamePhase::from_u32(self.phase)
    }

    pub fn previous_phase(&self) -> Option<GamePhase> {
        (self.previous_phase != GAME_PHASE_INVALID)
            .then(|| GamePhase::from_u32(self.previous_phase))
    }

    pub fn tick(&mut self, dt: f32) -> Option<GamePhase> {
        self.advance(dt, true)
    }

    /// Advances like [`Self::tick`], but `match_duration` running out does
    /// not end the match: the scene's Match Rules own the match clock and
    /// end it through `finish_match_requested`.
    pub fn tick_under_match_rules(&mut self, dt: f32) -> Option<GamePhase> {
        self.advance(dt, false)
    }

    fn advance(&mut self, dt: f32, own_timeout: bool) -> Option<GamePhase> {
        self.phase = self.current_phase().as_u32();
        self.phase_elapsed += dt.max(0.0);

        if self.current_phase() == GamePhase::Playing {
            self.match_time_remaining = (self.match_time_remaining - dt.max(0.0)).max(0.0);
        }

        let next_phase = self.next_phase(own_timeout);
        self.clear_one_shot_requests();

        next_phase.and_then(|phase| self.transition_to(phase).then_some(phase))
    }

    pub fn transition_to(&mut self, phase: GamePhase) -> bool {
        if self.current_phase() == phase {
            return false;
        }

        self.previous_phase = self.current_phase().as_u32();
        self.phase = phase.as_u32();
        self.phase_elapsed = 0.0;

        match phase {
            GamePhase::MainMenu => {
                self.players_ready = false;
                self.match_time_remaining = self.match_duration.max(0.0);
                self.winning_team = GAME_PHASE_INVALID;
            }
            GamePhase::Lobby => {
                self.players_ready = false;
                self.match_time_remaining = self.match_duration.max(0.0);
                self.winning_team = GAME_PHASE_INVALID;
            }
            GamePhase::Playing => {
                self.match_time_remaining = self.match_duration.max(0.0);
                self.winning_team = GAME_PHASE_INVALID;
            }
            GamePhase::GameOver => {}
        }

        true
    }

    fn next_phase(&self, own_timeout: bool) -> Option<GamePhase> {
        if self.return_to_menu_requested {
            return Some(GamePhase::MainMenu);
        }

        match self.current_phase() {
            GamePhase::MainMenu => self.enter_lobby_requested.then_some(GamePhase::Lobby),
            GamePhase::Lobby => {
                (self.start_match_requested && self.players_ready).then_some(GamePhase::Playing)
            }
            GamePhase::Playing => (self.finish_match_requested
                || (own_timeout
                    && self.match_duration > 0.0
                    && self.match_time_remaining <= f32::EPSILON))
                .then_some(GamePhase::GameOver),
            GamePhase::GameOver => self.restart_match_requested.then_some(GamePhase::Lobby),
        }
    }

    fn clear_one_shot_requests(&mut self) {
        self.enter_lobby_requested = false;
        self.start_match_requested = false;
        self.finish_match_requested = false;
        self.restart_match_requested = false;
        self.return_to_menu_requested = false;
    }
}

impl ComponentUpdate for ComponentGameState {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_game_state(scene, this.game_object(), resources.time().delta_time());
    }
}

/// Advances the scene's Game State. When the scene has Match Rules, they
/// decide when the match ends, so the Game State's own timeout is off.
/// Leaving a match for the lobby or the menu resets the arena for the next
/// one: see [`reset_combat`].
pub fn update_game_state(scene: &mut Scene, game_object: GameObject, dt: f32) -> Option<GamePhase> {
    let ruled = crate::match_rules::match_rules(scene).is_some();
    let mut transition = None;
    let mut left_match = false;
    let _ = scene.write_component::<ComponentGameState, _>(game_object, |state| {
        let from = state.current_phase();
        transition = if ruled {
            state.tick_under_match_rules(dt)
        } else {
            state.tick(dt)
        };
        left_match = matches!(from, GamePhase::Playing | GamePhase::GameOver)
            && matches!(transition, Some(GamePhase::Lobby | GamePhase::MainMenu));
    });
    if left_match {
        reset_combat(scene);
    }
    transition
}

/// The phase of the scene's Game State, or `None` in a scene without one.
pub fn game_phase(scene: &Scene) -> Option<GamePhase> {
    <&ComponentGameState>::query()
        .iter(scene.world())
        .next()
        .map(ComponentGameState::current_phase)
}

/// Whether combat is stopped: outside a Game State's Playing phase, and once
/// the match is decided. Tanks, sentries, rounds and respawns then hold still,
/// and no damage or kill is recorded. A scene without a Game State fights
/// until its match is decided.
pub fn combat_paused(scene: &Scene) -> bool {
    crate::match_rules::match_over(scene)
        || game_phase(scene).is_some_and(|phase| phase != GamePhase::Playing)
}

/// Clears the arena for the next match: rounds still in flight go, and every
/// tank is back at its spawn point, alive, at full health and ammo, and
/// ready to move the moment the match starts. Kills, deaths, assists and
/// score are reset by the scene's Match Rules.
pub fn reset_combat(scene: &mut Scene) {
    let rounds: Vec<_> = scene.objects_with::<ComponentProjectile>().collect();
    for round in rounds {
        scene.delete(round);
    }
    let tanks: Vec<_> = crate::vision::tank_combatants(scene).collect();
    for tank in tanks {
        crate::spawn::reset_for_match(scene, tank);
        let _ = scene.write_component::<ComponentHealth, _>(tank, ComponentHealth::restore_full);
        let _ = scene.write_component::<ComponentTankController, _>(tank, |controller| {
            controller.ammo = controller.max_ammo;
            controller.fire_cooldown_remaining = 0.0;
            controller.reload_remaining = 0.0;
            controller.speed = 0.0;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{combat_paused, update_game_state, ComponentGameState, GamePhase};
    use crate::spawn::{ComponentRespawnState, ComponentSpawnPoint};
    use crate::tank::{ComponentHealth, ComponentProjectile, ComponentTankController};
    use calyx_engine::math::Transform;

    #[test]
    fn combat_is_paused_outside_playing() {
        let mut scene = calyx_engine::test_support::test_scene();
        assert!(
            !combat_paused(&scene),
            "a scene without a Game State fights"
        );
        let state = scene.create(None, None);
        scene.add_component(state, ComponentGameState::default());
        for (phase, paused) in [
            (GamePhase::MainMenu, true),
            (GamePhase::Lobby, true),
            (GamePhase::Playing, false),
            (GamePhase::GameOver, true),
        ] {
            scene.write_component::<ComponentGameState, _>(state, |state| {
                state.phase = phase.as_u32();
            });
            assert_eq!(combat_paused(&scene), paused, "{phase:?}");
        }
    }

    #[test]
    fn leaving_a_match_clears_rounds_and_restores_tanks_at_their_spawn() {
        let mut scene = calyx_engine::test_support::test_scene();
        let state = scene.create(None, None);
        scene.add_component(
            state,
            ComponentGameState {
                phase: GamePhase::Playing.as_u32(),
                ..Default::default()
            },
        );
        let spawn = scene.create(None, None);
        scene.add_component(spawn, ComponentSpawnPoint::default());
        scene.set_world_transform(spawn, Transform::from_xyz(5.0, 0.0, 7.0).matrix());
        let tank = scene.create(None, None);
        scene.add_component(
            tank,
            ComponentTankController {
                ammo: 0,
                max_ammo: 6,
                ..Default::default()
            },
        );
        scene.add_component(
            tank,
            ComponentHealth {
                current_health: 0.0,
                dead: true,
                ..Default::default()
            },
        );
        scene.add_component(
            tank,
            ComponentRespawnState {
                alive: false,
                respawn_remaining: 2.0,
                ..Default::default()
            },
        );
        let round = scene.create(None, None);
        scene.add_component(round, ComponentProjectile::default());

        let finish = |scene: &mut calyx_engine::scene::Scene,
                      request: fn(&mut ComponentGameState)| {
            scene.write_component::<ComponentGameState, _>(state, request);
            update_game_state(scene, state, 0.1)
        };
        assert_eq!(
            finish(&mut scene, |state| state.finish_match_requested = true),
            Some(GamePhase::GameOver)
        );
        scene.flush_deletes();
        assert!(
            scene.find(scene.uuid(round)).is_some(),
            "the end keeps rounds"
        );

        assert_eq!(
            finish(&mut scene, |state| state.restart_match_requested = true),
            Some(GamePhase::Lobby)
        );
        scene.flush_deletes();
        assert!(scene.objects().all(|object| object != round));
        assert_eq!(
            scene.world_transform(tank).position,
            nalgebra_glm::vec3(5.0, 0.0, 7.0)
        );
        let respawn = scene
            .read_component::<ComponentRespawnState, _, _>(tank, |state| *state)
            .unwrap();
        assert!(respawn.alive && !respawn.is_gameplay_locked());
        assert_eq!(
            scene.read_component::<ComponentHealth, _, _>(tank, |h| (h.current_health, h.dead)),
            Some((100.0, false))
        );
        assert_eq!(
            scene.read_component::<ComponentTankController, _, _>(tank, |c| c.ammo),
            Some(6)
        );
    }

    #[test]
    fn defaults_to_main_menu() {
        let state = ComponentGameState::default();

        assert_eq!(state.current_phase(), GamePhase::MainMenu);
        assert_eq!(state.previous_phase(), None);
        assert_eq!(state.match_time_remaining, state.match_duration);
    }

    #[test]
    fn advances_through_menu_lobby_playing_and_game_over() {
        let mut state = ComponentGameState {
            enter_lobby_requested: true,
            ..Default::default()
        };
        assert_eq!(state.tick(0.25), Some(GamePhase::Lobby));
        assert_eq!(state.current_phase(), GamePhase::Lobby);
        assert_eq!(state.previous_phase(), Some(GamePhase::MainMenu));

        state.players_ready = true;
        state.start_match_requested = true;
        assert_eq!(state.tick(0.25), Some(GamePhase::Playing));
        assert_eq!(state.current_phase(), GamePhase::Playing);

        state.finish_match_requested = true;
        assert_eq!(state.tick(0.25), Some(GamePhase::GameOver));
        assert_eq!(state.current_phase(), GamePhase::GameOver);

        state.restart_match_requested = true;
        assert_eq!(state.tick(0.25), Some(GamePhase::Lobby));
        assert_eq!(state.current_phase(), GamePhase::Lobby);
    }

    #[test]
    fn lobby_waits_for_players_before_starting_match() {
        let mut state = ComponentGameState {
            phase: GamePhase::Lobby.as_u32(),
            start_match_requested: true,
            players_ready: false,
            ..Default::default()
        };

        assert_eq!(state.tick(0.0), None);
        assert_eq!(state.current_phase(), GamePhase::Lobby);
    }

    #[test]
    fn playing_match_timer_reaches_game_over() {
        let mut state = ComponentGameState {
            phase: GamePhase::Playing.as_u32(),
            match_duration: 5.0,
            match_time_remaining: 1.0,
            ..Default::default()
        };

        assert_eq!(state.tick(1.25), Some(GamePhase::GameOver));
        assert_eq!(state.current_phase(), GamePhase::GameOver);
        assert_eq!(state.match_time_remaining, 0.0);
    }

    #[test]
    fn return_to_menu_overrides_active_phase() {
        let mut state = ComponentGameState {
            phase: GamePhase::Playing.as_u32(),
            return_to_menu_requested: true,
            ..Default::default()
        };

        assert_eq!(state.tick(0.1), Some(GamePhase::MainMenu));
        assert_eq!(state.current_phase(), GamePhase::MainMenu);
        assert!(!state.return_to_menu_requested);
    }

    #[test]
    fn zero_duration_selects_an_elapsed_match_without_automatic_finish() {
        let mut state = ComponentGameState {
            phase: GamePhase::Playing.as_u32(),
            match_duration: 0.0,
            match_time_remaining: 0.0,
            ..Default::default()
        };
        assert_eq!(state.tick(2.0), None);
        assert_eq!(state.current_phase(), GamePhase::Playing);
        assert_eq!(state.phase_elapsed, 2.0);
        state.finish_match_requested = true;
        assert_eq!(state.tick(0.0), Some(GamePhase::GameOver));
    }
}
