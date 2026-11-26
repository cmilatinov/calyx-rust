use super::{
    combat_team, is_tank_combatant, record_damage, record_kill, Combatant, ComponentCombatStats,
    ComponentKillFeed, KillEvent, DAMAGE_FEEDBACK_LIFETIME,
};
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::ComponentTankController;
use calyx_engine::component::ComponentID;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::test_support::HeadlessSceneRunner;

fn named(scene: &mut Scene, name: &str) -> GameObject {
    scene.create(
        Some(ComponentID {
            name: name.to_string(),
            ..Default::default()
        }),
        None,
    )
}

fn event(scene: &Scene, killer: GameObject, victim: GameObject) -> KillEvent {
    KillEvent::new(scene, Some(killer), victim)
}

fn feed_text(feed: &ComponentKillFeed) -> Vec<String> {
    feed.entries()
        .map(|entry| format!("{:?} {}", entry.killer, entry.victim))
        .collect()
}

fn kill(victim: &str) -> KillEvent {
    KillEvent {
        killer: Some(Combatant {
            id: calyx_engine::utils::Uuid::new_v4(),
            name: "Tank".to_string(),
            team: None,
        }),
        victim: Combatant {
            id: calyx_engine::utils::Uuid::new_v4(),
            name: victim.to_string(),
            team: None,
        },
    }
}

#[test]
fn kill_feed_lists_the_newest_kill_first() {
    let mut feed = ComponentKillFeed::default();
    feed.push(&kill("Target A"));
    feed.advance(0.5);
    feed.push(&kill("Target B"));

    assert_eq!(
        feed_text(&feed),
        vec![
            "Some(\"Tank\") Target B".to_string(),
            "Some(\"Tank\") Target A".to_string()
        ]
    );
    let ages: Vec<f32> = feed.entries().map(|entry| entry.age).collect();
    assert_eq!(ages, vec![0.0, 0.5]);
}

#[test]
fn kill_feed_keeps_only_the_newest_max_entries() {
    let mut feed = ComponentKillFeed::default();
    for index in 0..7 {
        feed.push(&kill(&format!("Target {index}")));
    }

    let victims: Vec<&str> = feed.entries().map(|entry| entry.victim.as_str()).collect();
    assert_eq!(
        victims,
        vec!["Target 6", "Target 5", "Target 4", "Target 3", "Target 2"]
    );
}

#[test]
fn kill_feed_entries_fade_then_expire() {
    let mut feed = ComponentKillFeed {
        entry_lifetime: 4.0,
        fade_duration: 1.0,
        ..Default::default()
    };
    feed.push(&kill("Target A"));
    feed.advance(2.0);
    feed.push(&kill("Target B"));

    let opacities = |feed: &ComponentKillFeed| -> Vec<f32> {
        feed.entries().map(|entry| feed.opacity(entry)).collect()
    };
    assert_eq!(opacities(&feed), vec![1.0, 1.0]);

    feed.advance(1.5);
    assert_eq!(opacities(&feed), vec![1.0, 0.5], "Target A is fading");

    feed.advance(0.5);
    assert_eq!(
        feed_text(&feed),
        vec!["Some(\"Tank\") Target B".to_string()],
        "Target A expired after its 4 s lifetime"
    );

    feed.advance(2.0);
    assert_eq!(feed.entries().count(), 0);
}

#[test]
fn kill_feed_ages_with_the_simulation() {
    let mut scene = calyx_engine::test_support::test_scene();
    let hud = named(&mut scene, "Hud");
    scene.add_component(hud, ComponentKillFeed::default());
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.set_step_seconds(4.0);
    record_kill(runner.scene_mut(), &kill("Target A"));

    runner.step();
    let ages = |runner: &HeadlessSceneRunner| {
        runner
            .scene()
            .read_component::<ComponentKillFeed, _, _>(hud, |feed| {
                feed.entries().map(|entry| entry.age).collect::<Vec<_>>()
            })
            .unwrap()
    };
    assert_eq!(ages(&runner), vec![4.0]);
    runner.step();
    assert!(ages(&runner).is_empty(), "the entry expires after 6 s");
}

#[test]
fn recorded_kills_count_kills_and_deaths() {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = named(&mut scene, "Tank");
    let sentry = named(&mut scene, "Enemy Tank");
    let target = named(&mut scene, "Target A");
    scene.add_component(tank, ComponentCombatStats::default());
    scene.add_component(tank, ComponentTankController::default());
    scene.add_component(sentry, ComponentTankController::default());
    scene.add_component(
        sentry,
        ComponentCombatStats {
            team: 1,
            ..Default::default()
        },
    );

    let tank_kills_target = event(&scene, tank, target);
    let tank_kills_sentry = event(&scene, tank, sentry);
    let sentry_kills_tank = event(&scene, sentry, tank);
    record_kill(&mut scene, &tank_kills_target);
    record_kill(&mut scene, &tank_kills_sentry);
    record_kill(&mut scene, &sentry_kills_tank);

    let stats = |object| {
        scene
            .read_component::<ComponentCombatStats, _, _>(object, |stats| stats.clone())
            .unwrap()
    };
    assert_eq!(
        stats(tank),
        ComponentCombatStats {
            kills: 2,
            deaths: 1,
            score: 200,
            ..Default::default()
        }
    );
    assert_eq!(
        stats(sentry),
        ComponentCombatStats {
            kills: 1,
            deaths: 1,
            score: 100,
            team: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_self_kill_or_unknown_killer_counts_only_the_death() {
    let mut scene = calyx_engine::test_support::test_scene();
    let tank = named(&mut scene, "Tank");
    scene.add_component(tank, ComponentCombatStats::default());
    scene.add_component(tank, ComponentKillFeed::default());
    scene.add_component(tank, ComponentTankController::default());

    let self_kill = event(&scene, tank, tank);
    let environment = KillEvent::new(&scene, None, tank);
    record_kill(&mut scene, &self_kill);
    record_kill(&mut scene, &environment);

    assert_eq!(
        scene
            .read_component::<ComponentCombatStats, _, _>(tank, |stats| stats.clone())
            .unwrap(),
        ComponentCombatStats {
            kills: 0,
            deaths: 2,
            ..Default::default()
        }
    );
    let killers: Vec<Option<String>> = scene
        .read_component::<ComponentKillFeed, _, _>(tank, |feed| {
            feed.entries().map(|entry| entry.killer.clone()).collect()
        })
        .unwrap();
    assert_eq!(killers, vec![None, None]);
}

#[test]
fn recorded_kills_reach_every_kill_feed() {
    let mut scene = calyx_engine::test_support::test_scene();
    let first = named(&mut scene, "First HUD");
    let second = named(&mut scene, "Second HUD");
    scene.add_component(first, ComponentKillFeed::default());
    scene.add_component(second, ComponentKillFeed::default());

    record_kill(&mut scene, &kill("Target A"));

    for hud in [first, second] {
        let victims: Vec<String> = scene
            .read_component::<ComponentKillFeed, _, _>(hud, |feed| {
                feed.entries().map(|entry| entry.victim.clone()).collect()
            })
            .unwrap();
        assert_eq!(victims, vec!["Target A".to_string()]);
    }
}

fn combatants() -> (Scene, GameObject, GameObject, GameObject) {
    let mut scene = calyx_engine::test_support::test_scene();
    let killer = named(&mut scene, "Killer");
    let helper = named(&mut scene, "Helper");
    let victim = named(&mut scene, "Victim");
    for (object, team) in [(killer, 0), (helper, 0), (victim, 1)] {
        scene.add_component(object, ComponentTankController::default());
        scene.add_component(
            object,
            ComponentCombatStats {
                team,
                ..Default::default()
            },
        );
    }
    (scene, killer, helper, victim)
}

#[test]
fn damage_assists_score_and_feedback_follow_confirmed_events() {
    let (mut scene, killer, helper, victim) = combatants();
    super::record_damage(&mut scene, victim, Some(helper), 25.0, 100.0);
    super::record_damage(&mut scene, victim, Some(helper), 10.0, 100.0);
    super::record_damage(&mut scene, victim, Some(killer), 65.0, 100.0);
    let event = KillEvent::new(&scene, Some(killer), victim);
    record_kill(&mut scene, &event);
    let stats = |object| {
        scene
            .read_component::<ComponentCombatStats, _, _>(object, Clone::clone)
            .unwrap()
    };
    assert_eq!(
        (
            stats(killer).kills,
            stats(killer).assists,
            stats(killer).score
        ),
        (1, 0, 100)
    );
    assert_eq!(
        (
            stats(helper).kills,
            stats(helper).assists,
            stats(helper).score
        ),
        (0, 1, 50)
    );
    assert_eq!(stats(victim).deaths, 1);
    assert!(stats(helper)
        .feedback
        .iter()
        .all(|hit| hit.target == scene.uuid(victim)));
    assert!(stats(victim).contributions.is_empty());
    assert_eq!(
        stats(helper)
            .feedback
            .iter()
            .map(|hit| hit.amount)
            .collect::<Vec<_>>(),
        vec![25.0, 10.0]
    );
    assert!(
        stats(victim).feedback.is_empty(),
        "incoming damage does not confirm the victim's shots"
    );
}

#[test]
fn expired_assists_environment_and_team_damage_do_not_award_score() {
    for mode in 0..3 {
        let (mut scene, killer, helper, victim) = combatants();
        super::record_damage(&mut scene, victim, Some(helper), 25.0, 100.0);
        match mode {
            0 => {
                scene.write_component::<ComponentCombatStats, _>(victim, |stats| {
                    stats.advance(10.01)
                });
            }
            1 => {}
            _ => {
                scene.add_component(
                    victim,
                    crate::spawn::ComponentRespawnState {
                        team: 0,
                        ..Default::default()
                    },
                );
            }
        }
        let event = KillEvent::new(&scene, if mode == 1 { None } else { Some(killer) }, victim);
        record_kill(&mut scene, &event);
        assert_eq!(
            scene.read_component::<ComponentCombatStats, _, _>(helper, |stats| stats.assists),
            Some(0)
        );
        if mode != 0 {
            assert_eq!(
                scene.read_component::<ComponentCombatStats, _, _>(killer, |stats| stats.score),
                Some(0)
            );
        }
    }
}

#[test]
fn spawn_team_overrides_stats_and_inferred_side_and_effects_expire() {
    let (mut scene, killer, _, victim) = combatants();
    scene.add_component(killer, crate::tank::ComponentTankController::default());
    scene.add_component(
        killer,
        crate::spawn::ComponentRespawnState {
            team: 7,
            ..Default::default()
        },
    );
    assert_eq!(super::combat_team(&scene, killer), Some(7));
    super::record_damage(&mut scene, victim, Some(killer), 35.0, 100.0);
    scene.write_component::<ComponentCombatStats, _>(killer, |stats| {
        stats.advance(0.0);
        assert_eq!(stats.feedback[0].age, 0.0);
        stats.advance(0.5);
        assert_eq!(stats.feedback[0].age, 0.5);
        stats.advance(0.5);
        assert_eq!(
            stats.feedback.len(),
            1,
            "damage remains readable after one second"
        );
        stats.advance(DAMAGE_FEEDBACK_LIFETIME - 1.0);
        assert!(stats.feedback.is_empty());
    });
}

#[test]
fn sentry_kills_are_environmental_but_keep_the_neutral_feed_name() {
    let (mut scene, player, helper, victim) = combatants();
    let sentry = named(&mut scene, "Sentry");
    scene.add_component(sentry, ComponentSentry::default());
    // Authored stats or teams must never turn a hazard into a player.
    scene.add_component(
        sentry,
        ComponentCombatStats {
            team: 0,
            ..Default::default()
        },
    );
    scene.add_component(
        sentry,
        ComponentRespawnState {
            team: 0,
            ..Default::default()
        },
    );
    scene.add_component(player, ComponentKillFeed::default());
    record_damage(&mut scene, victim, Some(helper), 35.0, 100.0);
    record_damage(&mut scene, victim, Some(sentry), 65.0, 100.0);
    let event = KillEvent::new(&scene, Some(sentry), victim);
    record_kill(&mut scene, &event);

    for object in [player, helper, victim, sentry] {
        let stats = scene
            .read_component::<ComponentCombatStats, _, _>(object, Clone::clone)
            .unwrap();
        assert_eq!((stats.kills, stats.score, stats.assists), (0, 0, 0));
        assert_eq!(stats.deaths, u32::from(object == victim));
        assert!(stats.contributions.is_empty());
    }
    assert!(!is_tank_combatant(&scene, sentry));
    assert_eq!(combat_team(&scene, sentry), None);
    assert!(is_tank_combatant(&scene, player));
    let target = named(&mut scene, "Target A");
    scene.add_component(target, crate::tank::ComponentProjectileTarget::default());
    scene.add_component(target, ComponentCombatStats::default());
    assert!(!is_tank_combatant(&scene, target));
    let _ = scene.read_component::<ComponentKillFeed, _, _>(player, |feed| {
        let entry = feed.entries().next().unwrap();
        assert_eq!(entry.killer.as_deref(), Some("Sentry"));
        assert_eq!(entry.killer_team, None);
        assert_eq!(entry.victim, "Victim");
        assert_eq!(entry.victim_team, Some(1));
    });
}

#[test]
fn damageable_actors_join_stats_on_start_without_overwriting_authored_settings() {
    let mut scene = calyx_engine::test_support::test_scene();
    let target = scene.create(None, None);
    let authored = scene.create(None, None);
    for actor in [target, authored] {
        scene.add_component(actor, crate::tank::ComponentProjectileTarget::default());
    }
    scene.add_component(
        authored,
        ComponentCombatStats {
            team: 7,
            kill_score: 250,
            ..Default::default()
        },
    );
    let mut runner = HeadlessSceneRunner::from_scene(scene);
    runner.step();
    assert_eq!(
        runner
            .scene()
            .read_component::<ComponentCombatStats, _, _>(target, |stats| stats.kill_score),
        Some(100)
    );
    assert_eq!(
        runner
            .scene()
            .read_component::<ComponentCombatStats, _, _>(authored, |stats| (
                stats.team,
                stats.kill_score
            )),
        Some((7, 250))
    );
}
