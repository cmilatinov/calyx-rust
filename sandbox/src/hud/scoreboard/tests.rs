use super::*;
#[test]
fn sentry_kill_with_tank_contribution_adds_only_the_victim_death_to_scoreboard() {
    use crate::combat::{record_damage, record_kill, KillEvent};
    let mut scene = calyx_engine::test_support::test_scene();
    let attacker = scene.create(
        Some(calyx_engine::component::ComponentID {
            name: "Helper".into(),
            ..Default::default()
        }),
        None,
    );
    let victim = scene.create(
        Some(calyx_engine::component::ComponentID {
            name: "Victim".into(),
            ..Default::default()
        }),
        None,
    );
    for (object, team) in [(attacker, 0), (victim, 1)] {
        scene.add_component(object, crate::tank::ComponentTankController::default());
        scene.add_component(
            object,
            ComponentCombatStats {
                team,
                ..Default::default()
            },
        );
    }
    let sentry = scene.create(
        Some(calyx_engine::component::ComponentID {
            name: "Sentry".into(),
            ..Default::default()
        }),
        None,
    );
    scene.add_component(sentry, crate::sentry::ComponentSentry::default());
    scene.add_component(sentry, ComponentCombatStats::default());
    record_damage(&mut scene, victim, Some(attacker), 35.0, 100.0);
    record_damage(&mut scene, victim, Some(sentry), 65.0, 100.0);
    let event = KillEvent::new(&scene, Some(sentry), victim);
    record_kill(&mut scene, &event);
    let rows = scoreboard_rows(&scene, attacker);
    assert_eq!(rows.len(), 2);
    assert!(!rows.iter().any(|r| r.name == "Sentry"));
    for row in rows {
        assert_eq!((row.kills, row.score, row.assists), (0, 0, 0));
        assert_eq!(row.deaths, u32::from(row.name == "Victim"));
    }
}

#[test]
fn groups_sort_by_actual_score_then_kills_and_preserve_assists() {
    let mut scene = calyx_engine::test_support::test_scene();
    let player = scene.create(None, None);
    let friend = scene.create(None, None);
    let enemy = scene.create(None, None);
    for (object, team, score, kills, assists) in [
        (player, 0, 100, 1, 0),
        (friend, 0, 150, 1, 1),
        (enemy, 1, 300, 3, 0),
    ] {
        scene.add_component(object, crate::tank::ComponentTankController::default());
        scene.add_component(
            object,
            ComponentCombatStats {
                team,
                score,
                kills,
                assists,
                ..Default::default()
            },
        );
    }
    let sentry = scene.create(
        Some(calyx_engine::component::ComponentID {
            name: "Sentry".into(),
            ..Default::default()
        }),
        None,
    );
    scene.add_component(sentry, crate::sentry::ComponentSentry::default());
    scene.add_component(sentry, ComponentCombatStats::default());
    let target = scene.create(None, None);
    scene.add_component(target, crate::tank::ComponentProjectileTarget::default());
    scene.add_component(target, ComponentCombatStats::default());
    let rows = scoreboard_rows(&scene, player);
    assert_eq!(
        rows.iter()
            .map(|r| (r.friendly, r.score, r.assists))
            .collect::<Vec<_>>(),
        vec![(true, 150, 1), (true, 100, 0), (false, 300, 0)]
    );
    assert_eq!(grouped(12345), "12,345");
}
