//! Live combat statistics, grouped by side, and the result of a finished
//! match. Rendering never synthesizes rows.
use super::paint::{Painter, CYAN, LIGHT, MUTED, RED};
use crate::combat::{combat_team, is_tank_combatant, same_side, ComponentCombatStats};
use crate::match_rules::{match_mode, match_rules, MatchEnd, MatchMode, MatchWinner};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{TextAnchor, UiColor};

/// Rows a team section shows per page; free for all's single list shows two
/// sections' worth.
const ROWS_PER_SECTION: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ScoreboardRow {
    name: String,
    kills: u32,
    deaths: u32,
    assists: u32,
    score: u32,
    friendly: bool,
    is_player: bool,
}

/// How a finished match ended for the HUD's player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Verdict {
    Victory,
    Defeat,
    Draw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MatchResult {
    pub verdict: Verdict,
    pub end: MatchEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Scoreboard {
    pub rows: Vec<ScoreboardRow>,
    /// Team deathmatch shows a friendly and an opposing section; free for
    /// all ranks every tank in one list.
    pub teams: bool,
    /// Set once the match is over.
    pub result: Option<MatchResult>,
}

impl Scoreboard {
    pub(super) fn in_scene(scene: &Scene, player: GameObject) -> Self {
        let teams = match_mode(scene) == MatchMode::TeamDeathmatch;
        let mut rows = scoreboard_rows(scene, player);
        if !teams {
            rows.sort_by(rank_order);
        }
        Self {
            rows,
            teams,
            result: match_result(scene, player),
        }
    }

    /// The sections this page shows: each one's side, the rank before its
    /// first row, and its rows.
    fn sections(&self, page: usize) -> Vec<(bool, usize, Vec<&ScoreboardRow>)> {
        if self.teams {
            [true, false]
                .into_iter()
                .map(|friendly| {
                    let rows = self
                        .rows
                        .iter()
                        .filter(|r| r.friendly == friendly)
                        .skip(page * ROWS_PER_SECTION)
                        .take(ROWS_PER_SECTION)
                        .collect();
                    (friendly, page * ROWS_PER_SECTION, rows)
                })
                .collect()
        } else {
            let per_page = ROWS_PER_SECTION * 2;
            let rows = self
                .rows
                .iter()
                .skip(page * per_page)
                .take(per_page)
                .collect::<Vec<_>>();
            let leader_friendly = self.rows.first().is_none_or(|row| row.friendly);
            vec![(leader_friendly, page * per_page, rows)]
        }
    }

    pub(super) fn page_count(&self) -> usize {
        if self.teams {
            page_count(&self.rows)
        } else {
            self.rows.len().div_ceil(ROWS_PER_SECTION * 2).max(1)
        }
    }
}

/// Most kills first, since kills decide the match, then score, fewest deaths
/// and name.
fn rank_order(a: &ScoreboardRow, b: &ScoreboardRow) -> std::cmp::Ordering {
    b.kills
        .cmp(&a.kills)
        .then(b.score.cmp(&a.score))
        .then(a.deaths.cmp(&b.deaths))
        .then(a.name.cmp(&b.name))
}

pub(super) fn match_result(scene: &Scene, player: GameObject) -> Option<MatchResult> {
    let outcome = match_rules(scene)?.outcome()?;
    let verdict = match outcome.winner {
        MatchWinner::Draw => Verdict::Draw,
        MatchWinner::Player(id) if id == scene.uuid(player) => Verdict::Victory,
        MatchWinner::Team(team) if Some(team) == combat_team(scene, player) => Verdict::Victory,
        _ => Verdict::Defeat,
    };
    Some(MatchResult {
        verdict,
        end: outcome.end,
    })
}

pub(super) fn scoreboard_rows(scene: &Scene, player: GameObject) -> Vec<ScoreboardRow> {
    let mut rows = scene
        .objects()
        .filter(|&object| is_tank_combatant(scene, object))
        .filter_map(|object| {
            scene.read_component::<ComponentCombatStats, _, _>(object, |stats| ScoreboardRow {
                name: scene.name(object),
                kills: stats.kills,
                deaths: stats.deaths,
                assists: stats.assists,
                score: stats.score,
                friendly: object == player || same_side(scene, player, object),
                is_player: object == player,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.friendly.cmp(&a.friendly).then(rank_order(a, b)));
    rows
}

pub(super) fn page_count(rows: &[ScoreboardRow]) -> usize {
    [true, false]
        .iter()
        .map(|side| {
            rows.iter()
                .filter(|row| row.friendly == *side)
                .count()
                .div_ceil(ROWS_PER_SECTION)
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

fn grouped(value: u32) -> String {
    let text = value.to_string();
    let mut result = String::new();
    for (i, c) in text.chars().enumerate() {
        if i > 0 && (text.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

pub(super) fn paint_scoreboard(
    p: &mut Painter,
    board: &Scoreboard,
    width: f32,
    height: f32,
    page: usize,
    reserve: f32,
) {
    p.backdrop(width, height);
    let unit = super::menu::unit(height);
    // The end screen's buttons take a strip at the bottom; the board centres
    // in the rest.
    let height = height - reserve;
    let w = (width * 0.83).clamp(560.0, 1500.0).min(width - 36.0);
    let x = (width - w) * 0.5;
    let pages = board.page_count();
    let page = page.min(pages - 1);
    let sections = board.sections(page);
    let count = sections
        .iter()
        .map(|(_, _, rows)| rows.len().max(1))
        .sum::<usize>();
    let padding = 32.0 * unit;
    let gap = 30.0 * unit;
    let header = 115.0 * unit;
    let footer = 35.0 * unit;
    let gaps = gap * sections.len().saturating_sub(1) as f32;
    let banner_h = if board.result.is_some() {
        96.0 * unit
    } else {
        0.0
    };
    let row_h = ((height * 0.76 - header - footer - gaps - banner_h) / count as f32)
        .clamp(26.0, 66.0 * unit);
    let h = header + footer + gaps + count as f32 * row_h;
    let y = (height - h - banner_h) * 0.5 + 10.0 * unit;
    p.panel(x, y, w, h, 30.0 * unit);
    let title_w = (w * 0.38).max(260.0 * unit);
    let title_h = 86.0 * unit;
    let title_x = x + (w - title_w) * 0.5;
    let title_y = y - title_h * 0.48;
    // The title overlaps the main bevel, with a broad soft top glow.
    p.title_plate(title_x, title_y, title_w, title_h);
    p.gradient(
        &[
            (title_x + title_w * 0.18, title_y + 2.0),
            (title_x + title_w * 0.5, title_y + 2.0),
            (title_x + title_w * 0.82, title_y + 2.0),
            (title_x + title_w * 0.18, title_y + 12.0),
            (title_x + title_w * 0.5, title_y + 12.0),
            (title_x + title_w * 0.82, title_y + 12.0),
        ],
        &[
            CYAN.with_alpha(0.0),
            CYAN.with_alpha(0.3),
            CYAN.with_alpha(0.0),
            CYAN.with_alpha(0.0),
            CYAN.with_alpha(0.0),
            CYAN.with_alpha(0.0),
        ],
        &[0, 1, 4, 0, 4, 3, 1, 2, 5, 1, 5, 4],
    );
    p.middle(
        x + w * 0.5,
        title_y + title_h * 0.5,
        "SCOREBOARD",
        46.0 * unit,
        LIGHT,
        Some(title_w - 40.0),
    );
    let left = x + padding;
    let inner = w - padding * 2.0;
    let rank_x = left + inner * 0.065;
    let icon_x = left + inner * 0.13;
    let name_x = left + inner * 0.195;
    let columns = [
        left + inner * 0.615,
        left + inner * 0.71,
        left + inner * 0.805,
        left + inner * 0.962,
    ];
    let hy = y + header * 0.68;
    let font = (row_h * 0.5).clamp(16.0, 32.0 * unit);
    p.middle(rank_x, hy, "#", 25.0 * unit, MUTED, None);
    p.label(
        icon_x,
        hy,
        "PLAYER",
        25.0 * unit,
        LIGHT,
        TextAnchor::CenterLeft,
        None,
        false,
        None,
    );
    let sym = unit * 1.15;
    p.tank(columns[0] - 25.5 * sym, hy - 12.0 * sym, sym, LIGHT);
    p.skull(columns[1] - 11.5 * sym, hy - 12.5 * sym, sym, LIGHT);
    p.assist(columns[2] - 16.5 * sym, hy - 11.5 * sym, sym, LIGHT);
    p.label(
        columns[3],
        hy,
        "SCORE",
        25.0 * unit,
        LIGHT,
        TextAnchor::CenterRight,
        None,
        false,
        None,
    );
    let mut top = y + header;
    for (friendly, first_rank, group) in &sections {
        let friendly = *friendly;
        let n = group.len().max(1);
        let gh = n as f32 * row_h;
        let color = if friendly { CYAN } else { RED };
        let leader_tint = if friendly {
            UiColor::rgba(25, 64, 81, 255)
        } else {
            UiColor::rgba(70, 35, 42, 255)
        };
        p.bevel(
            left,
            top,
            inner,
            gh,
            15.0 * unit,
            UiColor::rgba(77, 95, 108, 220),
        );
        p.bevel(
            left + 2.0,
            top + 2.0,
            inner - 4.0,
            gh - 4.0,
            14.0 * unit,
            UiColor::rgba(14, 23, 30, 250),
        );
        // Angled left edge: full team colour at the leader, fading down the section.
        p.gradient(
            &[
                (left + 16.0 * unit, top),
                (left + 32.0 * unit, top),
                (left + 13.0 * unit, top + 18.0 * unit),
                (left + 13.0 * unit, top + gh - 15.0 * unit),
                (left + 30.0 * unit, top + gh),
                (left + 16.0 * unit, top + gh),
                (left, top + gh - 16.0 * unit),
                (left, top + 17.0 * unit),
            ],
            &[
                color,
                color,
                color,
                color.with_alpha(0.25),
                color.with_alpha(0.2),
                color.with_alpha(0.2),
                color.with_alpha(0.25),
                color,
            ],
            &[0, 1, 2, 0, 2, 7, 7, 2, 3, 7, 3, 6, 6, 3, 4, 6, 4, 5],
        );
        if group.is_empty() {
            p.label(
                name_x,
                top + row_h * 0.5,
                if page == 0 {
                    "No combatants"
                } else {
                    "No more combatants"
                },
                font,
                MUTED,
                TextAnchor::CenterLeft,
                None,
                false,
                None,
            );
        }
        for (index, row) in group.iter().enumerate() {
            let ry = top + index as f32 * row_h;
            let cy = ry + row_h * 0.5;
            let leader = page == 0 && index == 0;
            // In free for all each row takes its own side's colour.
            let color = if row.friendly { CYAN } else { RED };
            if leader {
                p.gradient(
                    &[
                        (left + 20.0 * unit, ry + 2.0),
                        (left + inner - 16.0 * unit, ry + 2.0),
                        (left + inner - 2.0, ry + 16.0 * unit),
                        (left + inner - 2.0, ry + row_h - 2.0),
                        (left + 12.0 * unit, ry + row_h - 2.0),
                        (left + 12.0 * unit, ry + 16.0 * unit),
                    ],
                    &[
                        leader_tint,
                        UiColor::rgba(17, 26, 34, 255),
                        UiColor::rgba(17, 26, 34, 255),
                        UiColor::rgba(17, 26, 34, 255),
                        leader_tint,
                        leader_tint,
                    ],
                    &[0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5],
                );
                p.line(
                    (left + 25.0 * unit, ry + 2.0),
                    (left + inner - 18.0 * unit, ry + 2.0),
                    color.with_alpha(0.8),
                    2.0,
                );
                p.line(
                    (left + 14.0 * unit, ry + row_h - 2.0),
                    (left + inner - 3.0, ry + row_h - 2.0),
                    color.with_alpha(0.6),
                    2.0,
                );
            } else if index % 2 == 0 {
                p.fill(
                    left + 14.0 * unit,
                    ry,
                    inner - 17.0 * unit,
                    row_h,
                    UiColor::rgba(25, 35, 44, 255),
                    0.0,
                );
            }
            if row.is_player {
                p.polygon(
                    &[
                        (rank_x - 25.0 * unit, cy - 6.0 * unit),
                        (rank_x - 18.0 * unit, cy),
                        (rank_x - 25.0 * unit, cy + 6.0 * unit),
                    ],
                    color,
                );
            }
            p.middle(
                rank_x,
                cy,
                (first_rank + index + 1).to_string(),
                font,
                MUTED,
                None,
            );
            let ts = (row_h * 0.42) / 24.0;
            p.tank(icon_x - 25.5 * ts, cy - 12.0 * ts, ts, LIGHT);
            p.label(
                name_x,
                cy,
                &row.name,
                font * 1.1,
                color,
                TextAnchor::CenterLeft,
                Some(columns[0] - name_x - inner * 0.07),
                false,
                None,
            );
            for (column, value) in columns[..3]
                .iter()
                .zip([row.kills, row.deaths, row.assists])
            {
                p.middle(
                    *column,
                    cy,
                    value.to_string(),
                    font,
                    if leader { LIGHT } else { MUTED },
                    None,
                );
            }
            p.label(
                columns[3],
                cy,
                grouped(row.score),
                font,
                LIGHT,
                TextAnchor::CenterRight,
                None,
                false,
                None,
            );
        }
        top += gh + gap;
    }
    if let Some(result) = board.result {
        paint_result(p, result, x + w * 0.5, y + h + 22.0 * unit, unit);
    }
    if pages > 1 {
        p.middle(
            x + w * 0.5,
            y + h - footer * 0.45,
            format!("PAGE {}/{}  /  SCROLL", page + 1, pages),
            14.0 * unit,
            MUTED,
            None,
        );
    }
}

/// The plate under the final scoreboard naming the player's result and what
/// ended the match.
fn paint_result(p: &mut Painter, result: MatchResult, center_x: f32, top: f32, unit: f32) {
    let w = 460.0 * unit;
    let h = 74.0 * unit;
    let x = center_x - w * 0.5;
    p.panel(x, top, w, h, 18.0 * unit);
    let (verdict, color) = match result.verdict {
        Verdict::Victory => ("VICTORY", CYAN),
        Verdict::Defeat => ("DEFEAT", RED),
        Verdict::Draw => ("DRAW", LIGHT),
    };
    p.middle(
        center_x,
        top + h * 0.4,
        verdict,
        38.0 * unit,
        color,
        Some(w - 40.0),
    );
    let end = match result.end {
        MatchEnd::KillLimit => "KILL LIMIT REACHED",
        MatchEnd::TimeLimit => "TIME LIMIT REACHED",
    };
    p.middle(
        center_x,
        top + h * 0.78,
        end,
        15.0 * unit,
        MUTED,
        Some(w - 40.0),
    );
}

#[cfg(test)]
mod tests {
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
}
