//! Kill events and the state they feed: each combatant's kill and death
//! counts, and the kill feeds HUDs draw.
//!
//! Every kill goes through [`record_kill`], whatever reported it. Local
//! gameplay reports a lethal projectile hit; a networked game can deserialize
//! a [`KillEvent`] from the host and record it the same way.

use crate::sentry::ComponentSentry;
use crate::spawn::{ComponentRespawnState, SPAWN_TEAM_ANY};
use crate::tank::{ComponentProjectileTarget, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid, Uuid};
use nalgebra_glm::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Explicit spawn teams take precedence; unassigned sandbox actors use the two
/// sides of the local training match. Unknown actors have no implied allegiance.
pub fn combat_team(scene: &Scene, object: GameObject) -> Option<u32> {
    scene
        .read_component::<ComponentRespawnState, _, _>(object, |s| s.team)
        .filter(|team| *team != SPAWN_TEAM_ANY)
        .or_else(|| {
            scene
                .read_component::<ComponentCombatStats, _, _>(object, |s| s.team)
                .filter(|team| *team != SPAWN_TEAM_ANY)
        })
        .or_else(|| scene.read_component::<ComponentTankController, _, _>(object, |_| 0))
        .or_else(|| scene.read_component::<ComponentSentry, _, _>(object, |_| 1))
        .or_else(|| scene.read_component::<ComponentProjectileTarget, _, _>(object, |_| 1))
}

fn friendly(scene: &Scene, a: GameObject, b: GameObject) -> bool {
    combat_team(scene, a)
        .zip(combat_team(scene, b))
        .is_some_and(|(a, b)| a == b)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DamageFeedback {
    pub position: Vec3,
    pub amount: f32,
    pub max_health: f32,
    pub age: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Contribution {
    attacker: Uuid,
    age: f32,
}

/// Only call after health changed. This records removed HP, never requested
/// projectile damage, so immune/dead targets and overkill cannot invent feedback.
pub(crate) fn record_damage(
    scene: &mut Scene,
    target: GameObject,
    shooter: Option<GameObject>,
    amount: f32,
    max_health: f32,
) {
    if !amount.is_finite() || amount <= 0.0 {
        return;
    }
    let Some(shooter) = shooter.filter(|shooter| *shooter != target) else {
        return;
    };
    let position = scene.world_transform(target).position;
    let _ = scene.write_component::<ComponentCombatStats, _>(shooter, |stats| {
        stats.feedback.push(DamageFeedback {
            position,
            amount,
            max_health,
            age: 0.0,
        });
        // Bound rapid-fire effects independently of frame rate.
        if stats.feedback.len() > 32 {
            stats.feedback.remove(0);
        }
    });
    if friendly(scene, shooter, target) {
        return;
    }
    let attacker = scene.uuid(shooter);
    let _ = scene.write_component::<ComponentCombatStats, _>(target, |stats| {
        if let Some(entry) = stats
            .contributions
            .iter_mut()
            .find(|entry| entry.attacker == attacker)
        {
            entry.age = 0.0;
        } else {
            stats
                .contributions
                .push(Contribution { attacker, age: 0.0 });
        }
    });
}

/// One side of a kill: the game object's id and the name to show for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combatant {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub team: Option<u32>,
}

impl Combatant {
    pub fn of(scene: &Scene, game_object: GameObject) -> Self {
        Self {
            id: scene.uuid(game_object),
            name: scene.name(game_object),
            team: combat_team(scene, game_object),
        }
    }
}

/// `killer` destroyed `victim`. A kill with no killer, such as one the
/// environment caused, counts only the victim's death.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillEvent {
    pub killer: Option<Combatant>,
    pub victim: Combatant,
}

impl KillEvent {
    pub fn new(scene: &Scene, killer: Option<GameObject>, victim: GameObject) -> Self {
        Self {
            killer: killer.map(|killer| Combatant::of(scene, killer)),
            victim: Combatant::of(scene, victim),
        }
    }

    /// The killer when it is not the victim itself.
    fn credited_killer(&self) -> Option<&Combatant> {
        self.killer
            .as_ref()
            .filter(|killer| killer.id != self.victim.id)
    }
}

/// Records `event` in the scene: counts the kill and the death on the
/// combatants' [`ComponentCombatStats`], when they have one, and adds the
/// event to every [`ComponentKillFeed`].
pub fn record_kill(scene: &mut Scene, event: &KillEvent) {
    let victim = scene.find(event.victim.id);
    let killer = event
        .credited_killer()
        .and_then(|killer| scene.find(killer.id));
    let eligible_kill =
        killer.is_some_and(|killer| victim.is_none_or(|victim| !friendly(scene, killer, victim)));
    if let Some(killer) = event
        .credited_killer()
        .and_then(|killer| scene.find(killer.id))
    {
        let _ = scene.write_component::<ComponentCombatStats, _>(killer, |stats| {
            stats.kills = stats.kills.saturating_add(1);
            if eligible_kill {
                stats.score = stats.score.saturating_add(stats.kill_score);
            }
        });
    }
    if let Some(victim) = scene.find(event.victim.id) {
        let contributors = scene
            .write_component::<ComponentCombatStats, _>(victim, |stats| {
                stats.deaths = stats.deaths.saturating_add(1);
                std::mem::take(&mut stats.contributions)
                    .into_iter()
                    .filter(|entry| entry.age <= stats.assist_window.max(0.0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if eligible_kill {
            for entry in contributors {
                if let Some(attacker) = scene
                    .find(entry.attacker)
                    .filter(|a| Some(*a) != killer && *a != victim)
                {
                    if !friendly(scene, attacker, victim) {
                        let _ =
                            scene.write_component::<ComponentCombatStats, _>(attacker, |stats| {
                                stats.assists = stats.assists.saturating_add(1);
                                stats.score = stats.score.saturating_add(stats.assist_score);
                            });
                    }
                }
            }
        }
    }

    let feeds: Vec<GameObject> = scene
        .objects()
        .filter(|&object| {
            scene
                .read_component::<ComponentKillFeed, _, _>(object, |_| ())
                .is_some()
        })
        .collect();
    for feed in feeds {
        let _ = scene.write_component::<ComponentKillFeed, _>(feed, |feed| feed.push(event));
    }
}

/// Kills and deaths of one combatant. The scoreboard lists every object that
/// has one.
#[derive(Clone, Debug, PartialEq, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "f651c577-ddb4-40b5-8d7a-ac2b5ad663f0"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Combat Stats")]
#[serde(default)]
#[repr(C)]
pub struct ComponentCombatStats {
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub score: u32,
    /// Unassigned uses the spawn team or the sandbox actor's inferred side.
    pub team: u32,
    pub kill_score: u32,
    pub assist_score: u32,
    pub assist_window: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) feedback: Vec<DamageFeedback>,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) contributions: Vec<Contribution>,
}

impl Component for ComponentCombatStats {}

impl Default for ComponentCombatStats {
    fn default() -> Self {
        Self {
            kills: 0,
            deaths: 0,
            assists: 0,
            score: 0,
            team: SPAWN_TEAM_ANY,
            kill_score: 100,
            assist_score: 50,
            assist_window: 10.0,
            feedback: Vec::new(),
            contributions: Vec::new(),
        }
    }
}

impl ComponentCombatStats {
    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        for entry in &mut self.feedback {
            entry.age += dt;
        }
        self.feedback.retain(|entry| entry.age < 1.0);
        for entry in &mut self.contributions {
            entry.age += dt;
        }
        self.contributions
            .retain(|entry| entry.age <= self.assist_window.max(0.0));
    }
}

impl ComponentUpdate for ComponentCombatStats {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let _ = this.write(scene, |stats| stats.advance(resources.time().delta_time()));
    }
}

/// One line of a kill feed, `age` seconds after its kill.
#[derive(Clone, Debug, PartialEq)]
pub struct KillFeedEntry {
    pub killer: Option<String>,
    pub victim: String,
    pub age: f32,
    pub killer_team: Option<u32>,
    pub victim_team: Option<u32>,
}

/// The latest kills, newest first, for a HUD to draw. Each entry stays for
/// `entry_lifetime` seconds and fades out over the last `fade_duration` of
/// them; past `max_entries`, the oldest entry goes.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "867c33b3-9ee1-42d3-bffa-e5fa56b3bc40"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Kill Feed")]
#[serde(default)]
#[repr(C)]
pub struct ComponentKillFeed {
    #[reflect_attr(min = 1.0)]
    pub max_entries: u32,
    /// Seconds an entry stays in the feed.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub entry_lifetime: f32,
    /// Seconds an entry takes to fade out at the end of its lifetime.
    #[reflect_attr(min = 0.0, speed = 0.1)]
    pub fade_duration: f32,
    #[serde(skip)]
    #[reflect_skip]
    entries: VecDeque<KillFeedEntry>,
}

impl Default for ComponentKillFeed {
    fn default() -> Self {
        Self {
            max_entries: 5,
            entry_lifetime: 6.0,
            fade_duration: 1.5,
            entries: VecDeque::new(),
        }
    }
}

impl Component for ComponentKillFeed {}

impl ComponentUpdate for ComponentKillFeed {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        let dt = resources.time().delta_time();
        let _ = this.write(scene, |feed| feed.advance(dt));
    }
}

impl ComponentKillFeed {
    /// Adds `event` as the newest entry.
    pub fn push(&mut self, event: &KillEvent) {
        self.entries.push_front(KillFeedEntry {
            killer: event.credited_killer().map(|killer| killer.name.clone()),
            victim: event.victim.name.clone(),
            killer_team: event.credited_killer().and_then(|killer| killer.team),
            victim_team: event.victim.team,
            age: 0.0,
        });
        self.entries.truncate(self.max_entries.max(1) as usize);
    }

    /// Ages every entry by `dt` seconds and drops the expired ones.
    pub fn advance(&mut self, dt: f32) {
        let lifetime = self.entry_lifetime.max(0.0);
        for entry in &mut self.entries {
            entry.age += dt.max(0.0);
        }
        self.entries.retain(|entry| entry.age < lifetime);
    }

    /// The entries, newest first.
    pub fn entries(&self) -> impl Iterator<Item = &KillFeedEntry> {
        self.entries.iter()
    }

    /// How opaque `entry` draws: 1 until its fade starts, then down to 0 at
    /// the end of its lifetime.
    pub fn opacity(&self, entry: &KillFeedEntry) -> f32 {
        let remaining = self.entry_lifetime - entry.age;
        if self.fade_duration <= f32::EPSILON {
            return if remaining > 0.0 { 1.0 } else { 0.0 };
        }
        (remaining / self.fade_duration).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{record_kill, Combatant, ComponentCombatStats, ComponentKillFeed, KillEvent};
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
        let sentry = named(&mut scene, "Sentry");
        let target = named(&mut scene, "Target A");
        scene.add_component(tank, ComponentCombatStats::default());
        scene.add_component(sentry, ComponentCombatStats::default());

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
            assert!(stats.feedback.is_empty());
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
}
