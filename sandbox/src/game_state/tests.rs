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

    let finish = |scene: &mut calyx_engine::scene::Scene, request: fn(&mut ComponentGameState)| {
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
