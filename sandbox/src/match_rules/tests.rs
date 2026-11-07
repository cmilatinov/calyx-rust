use super::{
    match_over, update_match_rules, ComponentMatchRules, MatchEnd, MatchMode, MatchOutcome,
    MatchWinner, Standings, MATCH_MODE_FREE_FOR_ALL, MATCH_MODE_TEAM_DEATHMATCH,
};
use crate::combat::{record_damage, record_kill, same_side, ComponentCombatStats, KillEvent};
use crate::game_state::{ComponentGameState, GamePhase};
use crate::tank::ComponentTankController;
use calyx_engine::component::ComponentID;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::test_support::HeadlessSceneRunner;

fn tank(scene: &mut Scene, name: &str, team: u32) -> GameObject {
    let object = scene.create(
        Some(ComponentID {
            name: name.to_string(),
            ..Default::default()
        }),
        None,
    );
    scene.add_component(object, ComponentTankController::default());
    scene.add_component(
        object,
        ComponentCombatStats {
            team,
            ..Default::default()
        },
    );
    object
}

fn rules_object(scene: &mut Scene, rules: ComponentMatchRules) -> GameObject {
    let object = scene.create(None, None);
    scene.add_component(object, rules);
    object
}

fn rules(scene: &Scene, object: GameObject) -> ComponentMatchRules {
    scene
        .read_component::<ComponentMatchRules, _, _>(object, Clone::clone)
        .unwrap()
}

fn kills(scene: &Scene, object: GameObject) -> u32 {
    scene
        .read_component::<ComponentCombatStats, _, _>(object, |stats| stats.kills)
        .unwrap()
}

fn kill(scene: &mut Scene, killer: GameObject, victim: GameObject) {
    let event = KillEvent::new(scene, Some(killer), victim);
    record_kill(scene, &event);
}

#[test]
fn defaults_play_team_deathmatch_to_twenty_kills_in_five_minutes() {
    let rules = ComponentMatchRules::default();
    assert_eq!(rules.mode(), MatchMode::TeamDeathmatch);
    assert_eq!((rules.kill_limit, rules.time_limit), (20, 300.0));
    assert_eq!(rules.time_remaining(), Some(300.0));
    assert!(!rules.is_over());
    assert_eq!(
        MatchMode::from_u32(MATCH_MODE_FREE_FOR_ALL),
        MatchMode::FreeForAll
    );
}

#[test]
fn the_first_side_to_the_kill_limit_wins() {
    let mut rules = ComponentMatchRules {
        kill_limit: 3,
        ..Default::default()
    };
    let leading = Standings::new([(MatchWinner::Team(0), 2), (MatchWinner::Team(1), 1)]);
    assert_eq!(rules.advance(1.0, &leading), None);
    let reached = Standings::new([(MatchWinner::Team(0), 2), (MatchWinner::Team(1), 3)]);
    assert_eq!(
        rules.advance(1.0, &reached),
        Some(MatchOutcome {
            winner: MatchWinner::Team(1),
            end: MatchEnd::KillLimit,
        })
    );
    assert!(rules.is_over());
    // A decided match stays decided, and its clock stops.
    assert_eq!(rules.advance(500.0, &leading), None);
    assert_eq!(rules.elapsed(), 2.0);
}

#[test]
fn the_time_limit_goes_to_the_kill_leader_or_ends_in_a_draw() {
    let mut rules = ComponentMatchRules {
        time_limit: 10.0,
        ..Default::default()
    };
    let leading = Standings::new([(MatchWinner::Team(0), 4), (MatchWinner::Team(1), 5)]);
    assert_eq!(rules.advance(9.5, &leading), None);
    assert_eq!(rules.time_remaining(), Some(0.5));
    assert_eq!(
        rules.advance(0.5, &leading).map(|o| (o.winner, o.end)),
        Some((MatchWinner::Team(1), MatchEnd::TimeLimit))
    );
    assert_eq!(rules.time_remaining(), Some(0.0));

    rules.reset();
    let tied = Standings::new([(MatchWinner::Team(0), 5), (MatchWinner::Team(1), 5)]);
    assert_eq!(
        rules.advance(10.0, &tied).map(|o| o.winner),
        Some(MatchWinner::Draw)
    );
}

#[test]
fn zero_limits_never_end_the_match() {
    let mut rules = ComponentMatchRules {
        kill_limit: 0,
        time_limit: 0.0,
        ..Default::default()
    };
    let standings = Standings::new([(MatchWinner::Team(0), 1000)]);
    assert_eq!(rules.advance(100_000.0, &standings), None);
    assert_eq!(rules.time_remaining(), None);
}

#[test]
fn free_for_all_counts_each_tank_and_team_deathmatch_sums_teams() {
    let mut scene = calyx_engine::test_support::test_scene();
    let a = tank(&mut scene, "A", 0);
    let b = tank(&mut scene, "B", 0);
    let c = tank(&mut scene, "C", 1);
    for (object, kills) in [(a, 2), (b, 2), (c, 3)] {
        scene.write_component::<ComponentCombatStats, _>(object, |s| s.kills = kills);
    }
    let ffa = Standings::in_scene(&scene, MatchMode::FreeForAll);
    assert_eq!(ffa.top_kills(), 3);
    assert_eq!(ffa.winner(), MatchWinner::Player(scene.uuid(c)));
    let tdm = Standings::in_scene(&scene, MatchMode::TeamDeathmatch);
    assert_eq!(tdm.top_kills(), 4);
    assert_eq!(tdm.winner(), MatchWinner::Team(0));
}

#[test]
fn free_for_all_makes_teammates_enemies_who_earn_kills_and_assists() {
    let mut scene = calyx_engine::test_support::test_scene();
    let killer = tank(&mut scene, "Killer", 0);
    let helper = tank(&mut scene, "Helper", 0);
    let victim = tank(&mut scene, "Victim", 0);
    assert!(
        same_side(&scene, killer, victim),
        "without rules tanks of one team are allies"
    );
    rules_object(
        &mut scene,
        ComponentMatchRules {
            mode: MATCH_MODE_FREE_FOR_ALL,
            ..Default::default()
        },
    );
    assert!(!same_side(&scene, killer, victim));
    assert!(same_side(&scene, killer, killer));

    record_damage(&mut scene, victim, Some(helper), 40.0, 100.0);
    record_damage(&mut scene, victim, Some(killer), 60.0, 100.0);
    kill(&mut scene, killer, victim);
    let stats = |object| {
        scene
            .read_component::<ComponentCombatStats, _, _>(object, |s| (s.kills, s.assists, s.score))
            .unwrap()
    };
    assert_eq!(stats(killer), (1, 0, 100));
    assert_eq!(stats(helper), (0, 1, 50));
}

#[test]
fn a_team_kill_in_team_deathmatch_counts_no_kill() {
    let mut scene = calyx_engine::test_support::test_scene();
    let killer = tank(&mut scene, "Killer", 0);
    let ally = tank(&mut scene, "Ally", 0);
    rules_object(
        &mut scene,
        ComponentMatchRules {
            mode: MATCH_MODE_TEAM_DEATHMATCH,
            ..Default::default()
        },
    );
    kill(&mut scene, killer, ally);
    assert_eq!(kills(&scene, killer), 0);
    assert_eq!(
        scene.read_component::<ComponentCombatStats, _, _>(ally, |s| s.deaths),
        Some(1)
    );
}

#[test]
fn reaching_the_kill_limit_ends_the_match_and_freezes_the_scoreboard() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = tank(&mut scene, "Player", 0);
    let bot = tank(&mut scene, "Bot", 1);
    let rules = rules_object(
        &mut scene,
        ComponentMatchRules {
            kill_limit: 2,
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    kill(runner.scene_mut(), player, bot);
    runner.step();
    assert!(!match_over(runner.scene()));
    kill(runner.scene_mut(), player, bot);
    runner.step();
    assert!(match_over(runner.scene()));
    assert_eq!(
        self::rules(runner.scene(), rules).outcome(),
        Some(MatchOutcome {
            winner: MatchWinner::Team(0),
            end: MatchEnd::KillLimit,
        })
    );

    // Nothing more counts once the match is decided.
    kill(runner.scene_mut(), bot, player);
    assert_eq!(kills(runner.scene(), bot), 0);
    assert_eq!(
        runner
            .scene()
            .read_component::<ComponentCombatStats, _, _>(player, |s| s.deaths),
        Some(0)
    );
}

#[test]
fn the_match_clock_follows_the_game_state_phase() {
    let mut scene = calyx_engine::test_support::test_scene();
    tank(&mut scene, "Player", 0);
    let rules = rules_object(
        &mut scene,
        ComponentMatchRules {
            time_limit: 5.0,
            ..Default::default()
        },
    );
    let state = scene.create(None, None);
    scene.add_component(
        state,
        ComponentGameState {
            phase: GamePhase::Lobby.as_u32(),
            match_duration: 0.0,
            ..Default::default()
        },
    );

    assert_eq!(update_match_rules(&mut scene, rules, 10.0), None);
    assert_eq!(
        self::rules(&scene, rules).elapsed(),
        0.0,
        "no clock in the lobby"
    );

    scene.write_component::<ComponentGameState, _>(state, |s| {
        s.transition_to(GamePhase::Playing);
    });
    assert_eq!(update_match_rules(&mut scene, rules, 4.0), None);
    let outcome = update_match_rules(&mut scene, rules, 1.0).expect("time is up");
    assert_eq!(outcome.end, MatchEnd::TimeLimit);
    let finish = scene
        .read_component::<ComponentGameState, _, _>(state, |s| {
            (s.finish_match_requested, s.winning_team)
        })
        .unwrap();
    assert_eq!(finish, (true, 0));

    // Back in the lobby, the next match starts from a fresh clock.
    scene.write_component::<ComponentGameState, _>(state, |s| {
        s.transition_to(GamePhase::Lobby);
    });
    update_match_rules(&mut scene, rules, 1.0);
    assert!(!self::rules(&scene, rules).is_over());
    assert_eq!(self::rules(&scene, rules).elapsed(), 0.0);
}
