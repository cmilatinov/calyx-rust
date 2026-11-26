//! Kill events and the state they feed: each combatant's kill and death
//! counts, and the kill feeds HUDs draw.
//!
//! Every kill goes through [`record_kill`], whatever reported it. Local
//! gameplay reports a lethal projectile hit; a networked game can deserialize
//! a [`KillEvent`] from the host and record it the same way.

use crate::game_state::combat_paused;
use crate::match_rules::{match_mode, MatchMode};
use crate::sentry::ComponentSentry;
use crate::spawn::{ComponentRespawnState, SPAWN_TEAM_ANY};
use crate::tank::ComponentTankController;
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

/// Scoreboard participants are tanks, including bots using the tank controller.
/// A Sentry remains a hazard even if an author accidentally gives it a controller.
pub(crate) fn is_tank_combatant(scene: &Scene, object: GameObject) -> bool {
    scene
        .read_component::<ComponentTankController, _, _>(object, |_| ())
        .is_some()
        && scene
            .read_component::<ComponentSentry, _, _>(object, |_| ())
            .is_none()
}

/// Explicit tank spawn teams take precedence over stats teams. Unassigned tanks
/// use the local side; hazards and practice targets have no team allegiance.
pub fn combat_team(scene: &Scene, object: GameObject) -> Option<u32> {
    if !is_tank_combatant(scene, object) {
        return None;
    }
    scene
        .read_component::<ComponentRespawnState, _, _>(object, |s| s.team)
        .filter(|team| *team != SPAWN_TEAM_ANY)
        .or_else(|| {
            scene
                .read_component::<ComponentCombatStats, _, _>(object, |s| s.team)
                .filter(|team| *team != SPAWN_TEAM_ANY)
        })
        .or(Some(0))
}

/// Whether two tanks fight on the same side: one tank and itself always do,
/// two tanks of one team do only in team deathmatch. Hazards and practice
/// targets are on nobody's side.
pub fn same_side(scene: &Scene, a: GameObject, b: GameObject) -> bool {
    match (combat_team(scene, a), combat_team(scene, b)) {
        (Some(x), Some(y)) => a == b || (x == y && match_mode(scene) == MatchMode::TeamDeathmatch),
        _ => false,
    }
}

fn friendly(scene: &Scene, a: GameObject, b: GameObject) -> bool {
    same_side(scene, a, b)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DamageFeedback {
    pub target: Uuid,
    pub position: Vec3,
    pub amount: f32,
    pub max_health: f32,
    pub age: f32,
}

pub(crate) const DAMAGE_FEEDBACK_LIFETIME: f32 = 1.4;

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
    if !amount.is_finite() || amount <= 0.0 || combat_paused(scene) {
        return;
    }
    let Some(shooter) = shooter.filter(|shooter| *shooter != target) else {
        return;
    };
    let position = scene.world_transform(target).position;
    let target_id = scene.uuid(target);
    let _ = scene.write_component::<ComponentCombatStats, _>(shooter, |stats| {
        stats.feedback.push(DamageFeedback {
            target: target_id,
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
    if !is_tank_combatant(scene, shooter) || friendly(scene, shooter, target) {
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
    fn non_self_killer(&self) -> Option<&Combatant> {
        self.killer
            .as_ref()
            .filter(|killer| killer.id != self.victim.id)
    }
}

/// Records deaths and tank kill credit, then adds the event to every kill feed.
/// Hazards keep their display identity but award no kill, score or assist, and
/// a kill of the killer's own side counts only the death. Outside play and
/// once the match is over nothing is recorded, so the final scoreboard stays
/// as it ended.
/// A kill that reaches the kill limit ends the match at once, so a later kill
/// in the same frame cannot change the result.
pub fn record_kill(scene: &mut Scene, event: &KillEvent) {
    if combat_paused(scene) {
        return;
    }
    let victim = scene.find(event.victim.id);
    let killer = event
        .non_self_killer()
        .and_then(|killer| scene.find(killer.id))
        .filter(|killer| is_tank_combatant(scene, *killer));
    let eligible_kill =
        killer.is_some_and(|killer| victim.is_none_or(|victim| !friendly(scene, killer, victim)));
    if let Some(killer) = killer.filter(|_| eligible_kill) {
        let _ = scene.write_component::<ComponentCombatStats, _>(killer, |stats| {
            stats.kills = stats.kills.saturating_add(1);
            stats.score = stats.score.saturating_add(stats.kill_score);
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
                    if is_tank_combatant(scene, attacker) && !friendly(scene, attacker, victim) {
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

    let feeds: Vec<GameObject> = scene.objects_with::<ComponentKillFeed>().collect();
    for feed in feeds {
        let _ = scene.write_component::<ComponentKillFeed, _>(feed, |feed| feed.push(event));
    }
    if eligible_kill {
        crate::match_rules::settle_kill_limit(scene);
    }
}

/// Combat counters and transient feedback. Only tanks receive kill/score credit
/// or a scoreboard row; a hazard may retain this component for feedback storage.
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
    /// Clears one match's totals and damage credit, keeping the tank's team
    /// and scoring settings for the next match.
    pub fn reset_match(&mut self) {
        self.kills = 0;
        self.deaths = 0;
        self.assists = 0;
        self.score = 0;
        self.feedback.clear();
        self.contributions.clear();
    }

    pub(crate) fn advance(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        for entry in &mut self.feedback {
            entry.age += dt;
        }
        self.feedback
            .retain(|entry| entry.age < DAMAGE_FEEDBACK_LIFETIME);
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
    pub killer_id: Option<Uuid>,
    pub victim_id: Uuid,
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
            killer: event.non_self_killer().map(|killer| killer.name.clone()),
            killer_id: event.non_self_killer().map(|killer| killer.id),
            victim_id: event.victim.id,
            victim: event.victim.name.clone(),
            killer_team: event.non_self_killer().and_then(|killer| killer.team),
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
mod tests;
