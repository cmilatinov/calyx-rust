use engine::component::{
    Component, ComponentEventContext, ComponentUpdate, ReflectComponent, ReflectComponentUpdate,
};
use engine::input::Input;
use engine::reflect::{Reflect, ReflectDefault};
use engine::resource::ResourceMap;
use engine::scene::{GameObject, Scene};
use engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
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
#[repr(C)]
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
        self.phase = self.current_phase().as_u32();
        self.phase_elapsed += dt.max(0.0);

        if self.current_phase() == GamePhase::Playing {
            self.match_time_remaining = (self.match_time_remaining - dt.max(0.0)).max(0.0);
        }

        let next_phase = self.next_phase();
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

    fn next_phase(&self) -> Option<GamePhase> {
        if self.return_to_menu_requested {
            return Some(GamePhase::MainMenu);
        }

        match self.current_phase() {
            GamePhase::MainMenu => self.enter_lobby_requested.then_some(GamePhase::Lobby),
            GamePhase::Lobby => {
                (self.start_match_requested && self.players_ready).then_some(GamePhase::Playing)
            }
            GamePhase::Playing => (self.finish_match_requested
                || self.match_time_remaining <= f32::EPSILON)
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
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_game_state(scene, game_object, resources.time().delta_time());
    }
}

pub fn update_game_state(scene: &mut Scene, game_object: GameObject, dt: f32) -> Option<GamePhase> {
    let mut transition = None;
    let _ = scene.write_component::<ComponentGameState, _>(game_object, |state| {
        transition = state.tick(dt);
    });
    transition
}

#[cfg(test)]
mod tests {
    use super::{ComponentGameState, GamePhase};

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
}
