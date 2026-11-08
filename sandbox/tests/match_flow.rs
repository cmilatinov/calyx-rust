//! The arena's match flow, driven through its screens as a player would:
//! main menu, lobby, match, end screen, and rematch or back to the menu.
// Matches the crates under test: the particle system component nests deep
// enough that proving it `Send`/`Sync` passes the default limit.
#![recursion_limit = "256"]

use calyx_engine::scene::{GameObject, Scene, SceneHud};
use calyx_engine::test_support::{HeadlessSceneRunner, TestAssets};
use calyx_engine::ui::{PaintCommand, UiInput, UiPoint, UiRect, UiSize};
use nalgebra_glm::vec3;
use sandbox::combat::ComponentCombatStats;
use sandbox::game_state::{ComponentGameState, GamePhase};
use sandbox::hud::{menu_buttons, MenuAction, MenuScreen};
use sandbox::match_rules::{match_over, ComponentMatchRules};
use sandbox::spawn::ComponentRespawnState;
use sandbox::tank::{ComponentHealth, ComponentProjectile, ComponentTankController};

/// The headless runner's screen, which it binds as the game's input surface.
const SCREEN: (f32, f32) = (1280.0, 720.0);

fn sandbox_assets() -> (TestAssets, calyx_engine::context::AssetContext) {
    let copy = TestAssets::game();
    let assets = copy.asset_context();
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    (copy, assets)
}

/// The arena as authored, on its main menu.
fn load_arena(copy: &TestAssets, assets: &calyx_engine::context::AssetContext) -> Scene {
    assets
        .registries
        .assets
        .read()
        .reload_by_path::<Scene>(&copy.sandbox_root().join("game/scenes/arena.cxscene"))
        .expect("the arena should load")
        .read()
        .clone()
}

fn object_named(scene: &Scene, name: &str) -> GameObject {
    scene
        .objects()
        .find(|object| scene.name(*object) == name)
        .unwrap_or_else(|| panic!("the arena should contain {name}"))
}

fn phase(runner: &HeadlessSceneRunner) -> GamePhase {
    let scene = runner.scene();
    scene
        .read_component::<ComponentGameState, _, _>(object_named(scene, "Game State"), |state| {
            state.current_phase()
        })
        .expect("the arena should carry a Game State")
}

fn hud_text(scene: &Scene) -> Vec<String> {
    let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(SCREEN.0, SCREEN.1));
    let frame = SceneHud::default()
        .frame(scene, viewport, UiInput::default())
        .expect("the arena should draw a HUD");
    frame
        .paint_commands
        .iter()
        .filter_map(|command| match command {
            PaintCommand::Text { text, .. } => Some(text.clone()),
            PaintCommand::Label(label) => Some(label.text.clone()),
            _ => None,
        })
        .collect()
}

fn assert_shows(scene: &Scene, expected: &[&str]) {
    let text = hud_text(scene);
    for expected in expected {
        assert!(text.iter().any(|t| t == expected), "{expected} in {text:?}");
    }
}

/// Clicks `action`'s button on `screen` with the mouse, as a press and a
/// release a frame apart, then lets the Game State act on it: a frame for
/// the choice to reach the Game State, whichever hook updates first, and one
/// more for the objects a match reset deletes to go.
fn click(runner: &mut HeadlessSceneRunner, screen: MenuScreen, action: MenuAction) {
    let button = menu_buttons(screen, SCREEN.0, SCREEN.1)
        .into_iter()
        .find(|button| button.action == action)
        .unwrap_or_else(|| panic!("{screen:?} should offer {action:?}"));
    let (x, y) = button.center();
    let position = egui::pos2(x, y);
    runner.move_pointer(position);
    runner.step();
    runner.press_pointer_button(position, egui::PointerButton::Primary);
    runner.step();
    runner.release_pointer_button(position, egui::PointerButton::Primary);
    runner.step_many(3);
}

fn rounds(scene: &Scene) -> Vec<GameObject> {
    scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentProjectile, _, _>(object, |_| ())
                .is_some()
        })
        .collect()
}

fn kills(scene: &Scene, tank: GameObject) -> u32 {
    scene
        .read_component::<ComponentCombatStats, _, _>(tank, |stats| stats.kills)
        .unwrap()
}

fn health(scene: &Scene, object: GameObject) -> f32 {
    scene
        .read_component::<ComponentHealth, _, _>(object, |health| health.current_health)
        .unwrap()
}

/// Moves `target` onto the tank barrel's firing line, `distance` metres ahead.
fn place_on_firing_line(scene: &mut Scene, target: GameObject, distance: f32) {
    let tank = object_named(scene, "Tank");
    let barrel = scene
        .read_component::<ComponentTankController, _, _>(tank, |controller| {
            controller.barrel.game_object(scene)
        })
        .flatten()
        .expect("the tank should reference its barrel");
    let barrel_transform = scene.world_transform(barrel);
    let mut forward = barrel_transform.forward();
    forward.y = 0.0;
    let mut transform = scene.world_transform(target);
    transform.position = barrel_transform.position + forward.normalize() * distance;
    scene.set_world_transform(target, transform.matrix());
}

#[test]
fn the_arena_plays_menu_lobby_match_end_screen_and_rematch() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_arena(&copy, &assets);
    let rules = object_named(&scene, "Match Rules");
    scene.write_component::<ComponentMatchRules, _>(rules, |rules| rules.kill_limit = 1);
    let target = object_named(&scene, "Target A");
    let tank = object_named(&scene, "Tank");
    let spawn = scene.world_transform(object_named(&scene, "Player Spawn"));
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    // The arena opens on the main menu.
    runner.step();
    assert_eq!(phase(&runner), GamePhase::MainMenu);
    assert_shows(runner.scene(), &["TANK ARENA", "PLAY", "QUIT"]);

    click(&mut runner, MenuScreen::MainMenu, MenuAction::Play);
    assert_eq!(phase(&runner), GamePhase::Lobby);
    assert_shows(
        runner.scene(),
        &["LOBBY", "Tank", "NOT READY", "READY", "MAIN MENU"],
    );

    // Readying starts the match, without the click firing the gun.
    click(&mut runner, MenuScreen::Lobby, MenuAction::Ready);
    assert_eq!(phase(&runner), GamePhase::Playing);
    assert!(rounds(runner.scene()).is_empty(), "the Ready click fired");
    assert_shows(runner.scene(), &["05:00"]);

    // Destroying Target A reaches the kill limit and ends the match. The
    // turret follows the cursor, left over the Ready button: put the target
    // where it settles.
    runner.step_many(60);
    place_on_firing_line(runner.scene_mut(), target, 4.0);
    runner.press_key(egui::Key::Space);
    let mut steps = 0;
    while phase(&runner) != GamePhase::GameOver {
        runner.step();
        steps += 1;
        assert!(steps < 600, "destroying Target A should end the match");
    }
    runner.release_key(egui::Key::Space);
    runner.step();
    assert!(match_over(runner.scene()));
    assert_eq!(kills(runner.scene(), tank), 1);
    assert_shows(
        runner.scene(),
        &[
            "SCOREBOARD",
            "VICTORY",
            "KILL LIMIT REACHED",
            "REMATCH",
            "MAIN MENU",
        ],
    );

    // Rematch: back to the lobby, with the totals cleared and the tank back
    // at its spawn point.
    runner.press_key(egui::Key::W);
    runner.step_many(10);
    runner.release_key(egui::Key::W);
    click(&mut runner, MenuScreen::GameOver, MenuAction::Rematch);
    assert_eq!(phase(&runner), GamePhase::Lobby);
    assert!(!match_over(runner.scene()));
    assert_eq!(kills(runner.scene(), tank), 0);
    let at_spawn = runner.scene().world_transform(tank).position;
    assert!(
        (at_spawn.xz() - spawn.position.xz()).magnitude() < 1e-3,
        "the tank should be back at its spawn point, not {at_spawn:?}"
    );

    click(&mut runner, MenuScreen::Lobby, MenuAction::Ready);
    assert_eq!(phase(&runner), GamePhase::Playing);
    assert!(!match_over(runner.scene()), "the rematch is a new match");

    // Finish this one and leave for the main menu.
    let state = object_named(runner.scene(), "Game State");
    runner
        .scene_mut()
        .write_component::<ComponentGameState, _>(state, |state| {
            state.finish_match_requested = true;
        });
    runner.step();
    assert_eq!(phase(&runner), GamePhase::GameOver);
    click(&mut runner, MenuScreen::GameOver, MenuAction::MainMenu);
    assert_eq!(phase(&runner), GamePhase::MainMenu);
    assert_shows(runner.scene(), &["PLAY", "QUIT"]);

    // Enter picks each screen's main choice.
    runner.press_key(egui::Key::Enter);
    runner.step();
    runner.release_key(egui::Key::Enter);
    runner.step();
    assert_eq!(phase(&runner), GamePhase::Lobby);
}

#[test]
fn quit_asks_the_host_to_leave_play() {
    let (copy, assets) = sandbox_assets();
    let mut runner = HeadlessSceneRunner::from_scene(load_arena(&copy, &assets));
    runner.step();
    assert!(!runner.resources_mut().take_quit_request());
    click(&mut runner, MenuScreen::MainMenu, MenuAction::Quit);
    assert!(runner.resources_mut().take_quit_request());
    assert_eq!(phase(&runner), GamePhase::MainMenu);
}

#[test]
fn combat_is_frozen_outside_play() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_arena(&copy, &assets);
    let tank = object_named(&scene, "Tank");
    let player = object_named(&scene, "Player");
    let sentry = scene.world_transform(object_named(&scene, "Sentry"));
    // Ten metres in front of the sentry, which kills it within seconds in
    // play.
    let mut rig = scene.world_transform(player);
    rig.position = vec3(sentry.position.x, 0.0, sentry.position.z - 10.0);
    scene.set_world_transform(player, rig.matrix());
    let target = object_named(&scene, "Target A");
    place_on_firing_line(&mut scene, target, 4.0);
    let target_health = health(&scene, target);
    let mut runner = HeadlessSceneRunner::from_scene(scene);

    let still = |runner: &mut HeadlessSceneRunner, what: &str| {
        let position = runner.scene().world_transform(tank).position;
        runner.press_key(egui::Key::W);
        runner.press_key(egui::Key::Space);
        runner.step_many(300);
        runner.release_key(egui::Key::W);
        runner.release_key(egui::Key::Space);
        runner.step();
        let scene = runner.scene();
        assert_eq!(
            scene.world_transform(tank).position,
            position,
            "the tank moved in {what}"
        );
        assert!(rounds(scene).is_empty(), "rounds were fired in {what}");
        assert_eq!(health(scene, tank), 100.0, "the sentry hit in {what}");
        assert_eq!(
            health(scene, target),
            target_health,
            "the target was hit in {what}"
        );
    };
    still(&mut runner, "the main menu");
    click(&mut runner, MenuScreen::MainMenu, MenuAction::Play);
    assert_eq!(phase(&runner), GamePhase::Lobby);
    still(&mut runner, "the lobby");

    // A tank down in the lobby does not respawn until the match starts.
    runner
        .scene_mut()
        .write_component::<ComponentRespawnState, _>(tank, |state| {
            state.alive = false;
            state.respawn_remaining = 0.1;
        });
    runner.step_many(60);
    let alive = |runner: &HeadlessSceneRunner| {
        runner
            .scene()
            .read_component::<ComponentRespawnState, _, _>(tank, |state| state.alive)
            .unwrap()
    };
    assert!(!alive(&runner), "the tank respawned in the lobby");
    click(&mut runner, MenuScreen::Lobby, MenuAction::Ready);
    runner.step_many(10);
    assert!(alive(&runner), "the tank respawns once the match starts");

    // In play the tank drives again, once its spawn protection is over.
    let position = runner.scene().world_transform(tank).position;
    runner.press_key(egui::Key::W);
    runner.step_many(240);
    runner.release_key(egui::Key::W);
    runner.step();
    assert_ne!(runner.scene().world_transform(tank).position, position);
}

#[test]
fn rounds_in_flight_freeze_at_the_end_and_go_on_rematch() {
    let (copy, assets) = sandbox_assets();
    let mut scene = load_arena(&copy, &assets);
    // Nothing in the way of the rounds, which then fly their full range.
    for name in ["Sentry", "Target A", "Target B"] {
        let object = object_named(&scene, name);
        scene.delete(object);
    }
    scene.flush_deletes();
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    click(&mut runner, MenuScreen::MainMenu, MenuAction::Play);
    click(&mut runner, MenuScreen::Lobby, MenuAction::Ready);
    assert_eq!(phase(&runner), GamePhase::Playing);

    runner.press_key(egui::Key::Space);
    runner.step_many(3);
    runner.release_key(egui::Key::Space);
    runner.step();
    let in_flight = rounds(runner.scene());
    assert!(!in_flight.is_empty(), "the tank should have fired");

    // The match ends with the round in the air: it holds still there.
    let state = object_named(runner.scene(), "Game State");
    runner
        .scene_mut()
        .write_component::<ComponentGameState, _>(state, |state| {
            state.finish_match_requested = true;
        });
    runner.step();
    assert_eq!(phase(&runner), GamePhase::GameOver);
    let round = in_flight[0];
    let position = runner.scene().world_transform(round).position;
    runner.step_many(240);
    assert_eq!(rounds(runner.scene()), in_flight, "frozen rounds expired");
    assert_eq!(runner.scene().world_transform(round).position, position);

    // The rematch clears them, so they cannot resume in the next match.
    click(&mut runner, MenuScreen::GameOver, MenuAction::Rematch);
    assert_eq!(phase(&runner), GamePhase::Lobby);
    assert!(rounds(runner.scene()).is_empty());
    click(&mut runner, MenuScreen::Lobby, MenuAction::Ready);
    runner.step_many(60);
    assert!(rounds(runner.scene()).is_empty());
}
