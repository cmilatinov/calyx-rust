//! Match rules: the game mode, and the kill and time limits that end a match.
//!
//! [`ComponentMatchRules`] reads the kills [`crate::combat`] records. In free
//! for all every tank plays for itself and the first to the kill limit wins;
//! in team deathmatch a team's kills are the sum of its tanks' kills. When the
//! time limit runs out first, whoever leads on kills wins, and a tie on kills
//! is a draw. Once a match is decided, gameplay stops and the HUD shows the
//! final scoreboard.

use crate::combat::{combat_team, is_tank_combatant, ComponentCombatStats};
use crate::game_state::{ComponentGameState, GamePhase};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentRef, ComponentUpdate, ReflectComponent,
    ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid, Uuid};
use legion::IntoQuery;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MATCH_MODE_FREE_FOR_ALL: u32 = 0;
pub const MATCH_MODE_TEAM_DEATHMATCH: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchMode {
    /// Every tank is on its own side.
    FreeForAll,
    /// Tanks of one team are on the same side.
    TeamDeathmatch,
}

impl MatchMode {
    pub const fn from_u32(value: u32) -> Self {
        match value {
            MATCH_MODE_FREE_FOR_ALL => Self::FreeForAll,
            _ => Self::TeamDeathmatch,
        }
    }
}

/// Why a match ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchEnd {
    KillLimit,
    TimeLimit,
}

/// Who won a match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchWinner {
    /// The tank with this id, in free for all.
    Player(Uuid),
    /// This team, in team deathmatch.
    Team(u32),
    /// The leaders were tied on kills when time ran out.
    Draw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchOutcome {
    pub winner: MatchWinner,
    pub end: MatchEnd,
}

/// The rules of the match in this scene. A scene without one plays on with
/// teams and no limits.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "4d7c2e91-6a3b-4f58-9e0d-8b1f2c7a5e36"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate)]
#[reflect_attr(name = "Match Rules")]
#[serde(default)]
pub struct ComponentMatchRules {
    /// 0 is free for all, 1 is team deathmatch.
    #[reflect_attr(min = 0.0, max = 1.0)]
    pub mode: u32,
    /// Kills that win the match: one tank's in free for all, one team's in
    /// team deathmatch. Zero means no kill limit.
    #[reflect_attr(min = 0.0)]
    pub kill_limit: u32,
    /// Seconds the match lasts. Zero means no time limit.
    #[reflect_attr(min = 0.0, speed = 1.0)]
    pub time_limit: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) elapsed: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) outcome: Option<MatchOutcome>,
}

impl Default for ComponentMatchRules {
    fn default() -> Self {
        Self {
            mode: MATCH_MODE_TEAM_DEATHMATCH,
            kill_limit: 20,
            time_limit: 300.0,
            elapsed: 0.0,
            outcome: None,
        }
    }
}

impl Component for ComponentMatchRules {}

impl ComponentMatchRules {
    pub fn mode(&self) -> MatchMode {
        MatchMode::from_u32(self.mode)
    }

    /// Seconds played in this match.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Seconds left on the clock, or `None` without a time limit.
    pub fn time_remaining(&self) -> Option<f32> {
        (self.time_limit > 0.0).then(|| (self.time_limit - self.elapsed).max(0.0))
    }

    pub fn outcome(&self) -> Option<MatchOutcome> {
        self.outcome
    }

    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }

    /// Starts the match over: the clock from zero and no result.
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.outcome = None;
    }

    /// Advances the clock by `dt` and decides the match from `standings` once
    /// a limit is reached. Returns the outcome when this call ended the match.
    pub fn advance(&mut self, dt: f32, standings: &Standings) -> Option<MatchOutcome> {
        if self.is_over() {
            return None;
        }
        if dt.is_finite() {
            self.elapsed += dt.max(0.0);
        }
        let end = if self.kill_limit > 0 && standings.top_kills() >= self.kill_limit {
            MatchEnd::KillLimit
        } else if self.time_limit > 0.0 && self.elapsed >= self.time_limit {
            MatchEnd::TimeLimit
        } else {
            return None;
        };
        self.outcome = Some(MatchOutcome {
            winner: standings.winner(),
            end,
        });
        self.outcome
    }
}

/// Kills per side: per tank in free for all, per team in team deathmatch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Standings {
    sides: Vec<(MatchWinner, u32)>,
}

impl Standings {
    pub fn new(sides: impl IntoIterator<Item = (MatchWinner, u32)>) -> Self {
        Self {
            sides: sides.into_iter().collect(),
        }
    }

    /// The scene's tanks' kills, grouped into the sides of `mode`.
    pub fn in_scene(scene: &Scene, mode: MatchMode) -> Self {
        let mut teams = BTreeMap::<u32, u32>::new();
        let mut players = Vec::new();
        for object in scene.objects() {
            if !is_tank_combatant(scene, object) {
                continue;
            }
            let Some(kills) =
                scene.read_component::<ComponentCombatStats, _, _>(object, |stats| stats.kills)
            else {
                continue;
            };
            match mode {
                MatchMode::FreeForAll => {
                    players.push((MatchWinner::Player(scene.uuid(object)), kills));
                }
                MatchMode::TeamDeathmatch => {
                    let team = combat_team(scene, object).unwrap_or(0);
                    *teams.entry(team).or_default() += kills;
                }
            }
        }
        players.extend(
            teams
                .into_iter()
                .map(|(team, kills)| (MatchWinner::Team(team), kills)),
        );
        Self::new(players)
    }

    pub fn top_kills(&self) -> u32 {
        self.sides
            .iter()
            .map(|(_, kills)| *kills)
            .max()
            .unwrap_or(0)
    }

    /// The side with the most kills, or a draw when several share the lead.
    pub fn winner(&self) -> MatchWinner {
        let top = self.top_kills();
        let mut leaders = self.sides.iter().filter(|(_, kills)| *kills == top);
        match (leaders.next(), leaders.next()) {
            (Some((side, _)), None) => *side,
            _ => MatchWinner::Draw,
        }
    }
}

impl ComponentUpdate for ComponentMatchRules {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        _input: &Input,
    ) {
        update_match_rules(scene, this.game_object(), resources.time().delta_time());
    }
}

/// Runs the match clock and the limits for one frame. A scene's Game State,
/// when it has one, decides when the match is played: the clock only runs
/// while it is playing, a new match starts from its lobby or menu with the
/// clock and every tank's totals cleared, and the end of the match finishes
/// it. Kills made outside play, before the match starts, do not count: the
/// match's first playing frame clears them.
pub fn update_match_rules(
    scene: &mut Scene,
    game_object: GameObject,
    dt: f32,
) -> Option<MatchOutcome> {
    let phase = scene.objects().find_map(|object| {
        scene.read_component::<ComponentGameState, _, _>(object, |state| {
            (object, state.current_phase())
        })
    });
    if let Some((_, phase)) = phase {
        let played = scene
            .read_component::<ComponentMatchRules, _, _>(game_object, |rules| {
                rules.elapsed > 0.0 || rules.is_over()
            })
            .unwrap_or(false);
        if played && matches!(phase, GamePhase::MainMenu | GamePhase::Lobby) {
            let _ = scene.write_component::<ComponentMatchRules, _>(game_object, |rules| {
                rules.reset();
            });
            reset_match_stats(scene);
        }
        if phase != GamePhase::Playing {
            return None;
        }
        if !played {
            reset_match_stats(scene);
        }
    }
    let mode = scene.read_component::<ComponentMatchRules, _, _>(game_object, |rules| {
        (!rules.is_over()).then(|| rules.mode())
    })??;
    let standings = Standings::in_scene(scene, mode);
    let outcome = scene
        .write_component::<ComponentMatchRules, _>(game_object, |rules| {
            rules.advance(dt, &standings)
        })
        .flatten()?;
    if let Some((state, _)) = phase {
        let _ = scene.write_component::<ComponentGameState, _>(state, |state| {
            state.finish_match_requested = true;
            if let MatchWinner::Team(team) = outcome.winner {
                state.winning_team = team;
            }
        });
    }
    log::info!("Match over: {outcome:?}");
    Some(outcome)
}

/// The scene's match rules, if it has them.
pub fn match_rules(scene: &Scene) -> Option<ComponentMatchRules> {
    <&ComponentMatchRules>::query()
        .iter(scene.world())
        .next()
        .cloned()
}

/// The scene's game mode: team deathmatch unless its rules say otherwise.
pub fn match_mode(scene: &Scene) -> MatchMode {
    <&ComponentMatchRules>::query()
        .iter(scene.world())
        .next()
        .map_or(MatchMode::TeamDeathmatch, ComponentMatchRules::mode)
}

/// Whether the scene's match is decided. Gameplay stops once it is: tanks,
/// sentries, rounds and respawns hold still, and no more kills count.
pub fn match_over(scene: &Scene) -> bool {
    <&ComponentMatchRules>::query()
        .iter(scene.world())
        .any(ComponentMatchRules::is_over)
}

/// Clears every tank's kills, deaths, assists, score and damage credit, so
/// a rematch starts from zero.
fn reset_match_stats(scene: &mut Scene) {
    let tanks: Vec<_> = scene
        .objects()
        .filter(|&object| is_tank_combatant(scene, object))
        .collect();
    for tank in tanks {
        let _ = scene.write_component::<ComponentCombatStats, _>(tank, |stats| {
            stats.reset_match();
        });
    }
}

#[cfg(test)]
mod tests;
