//! Team-grouped live combat statistics. Rendering never synthesizes rows.
use super::paint::{clipped_name, Painter, BLACK, CYAN, LIGHT, MUTED, RED};
use crate::combat::{combat_team, ComponentCombatStats};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::UiColor;

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
pub(super) fn scoreboard_rows(scene: &Scene, player: GameObject) -> Vec<ScoreboardRow> {
    let team = combat_team(scene, player);
    let mut rows = scene
        .objects()
        .filter_map(|object| {
            scene.read_component::<ComponentCombatStats, _, _>(object, |stats| ScoreboardRow {
                name: scene.name(object),
                kills: stats.kills,
                deaths: stats.deaths,
                assists: stats.assists,
                score: stats.score,
                friendly: object == player
                    || team
                        .zip(combat_team(scene, object))
                        .is_some_and(|(a, b)| a == b),
                is_player: object == player,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.friendly
            .cmp(&a.friendly)
            .then(b.score.cmp(&a.score))
            .then(b.kills.cmp(&a.kills))
            .then(a.deaths.cmp(&b.deaths))
            .then(a.name.cmp(&b.name))
    });
    rows
}
pub(super) fn page_count(rows: &[ScoreboardRow]) -> usize {
    [true, false]
        .iter()
        .map(|side| {
            rows.iter()
                .filter(|row| row.friendly == *side)
                .count()
                .div_ceil(6)
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
    rows: &[ScoreboardRow],
    width: f32,
    height: f32,
    page: usize,
) {
    p.fill(0.0, 0.0, width, height, UiColor::rgba(3, 8, 15, 172), 0.0);
    let w = (width - 100.0).min(1330.0);
    let x = (width - w) * 0.5;
    let per_team = 6;
    let pages = page_count(rows);
    let page = page.min(pages - 1);
    let visible = rows
        .iter()
        .filter(|row| row.friendly)
        .skip(page * per_team)
        .take(per_team)
        .chain(
            rows.iter()
                .filter(|row| !row.friendly)
                .skip(page * per_team)
                .take(per_team),
        )
        .cloned()
        .collect::<Vec<_>>();
    let rows = &visible;
    let count = [true, false]
        .iter()
        .map(|side| {
            rows.iter()
                .filter(|row| row.friendly == *side)
                .count()
                .max(1)
        })
        .sum::<usize>();
    let row_h = ((height - 300.0) / count as f32).clamp(26.0, 60.0);
    let groups = 2.0;
    let h = 148.0 + count as f32 * row_h + groups * 14.0 + 22.0;
    let y = ((height - h) * 0.5).max(70.0);
    p.panel(x, y, w, h, 24.0);
    p.bevel(x + w * 0.32, y - 35.0, w * 0.36, 80.0, 18.0, BLACK);
    p.panel(x + w * 0.32, y - 35.0, w * 0.36, 80.0, 18.0);
    p.line(
        (x + w * 0.38, y - 33.0),
        (x + w * 0.62, y - 33.0),
        CYAN,
        2.0,
    );
    p.bold_centered(x + w * 0.5, y - 31.0, "SCOREBOARD", 46.0, LIGHT);
    let left = x + 24.0;
    let inner = w - 48.0;
    let columns = [
        left + inner * 0.53,
        left + inner * 0.65,
        left + inner * 0.77,
        left + inner * 0.91,
    ];
    p.text(left + 22.0, y + 70.0, "#", 25.0, LIGHT);
    p.text(left + 88.0, y + 70.0, "PLAYER", 25.0, LIGHT);
    p.tank(columns[0] - 25.0, y + 67.0, 1.5, LIGHT);
    p.skull(columns[1] - 15.0, y + 63.0, 1.3, LIGHT);
    p.assist(columns[2] - 22.0, y + 66.0, 1.4, LIGHT);
    p.centered(columns[3], y + 70.0, "SCORE", 25.0, LIGHT);
    if pages > 1 {
        p.centered(
            x + w * 0.5,
            y + h - 26.0,
            &format!("PAGE {}/{}  /  SCROLL", page + 1, pages),
            14.0,
            MUTED,
        );
    }
    let mut top = y + 115.0;
    for friendly in [true, false] {
        let team_rows = rows
            .iter()
            .filter(|row| row.friendly == friendly)
            .collect::<Vec<_>>();
        let color = if friendly { CYAN } else { RED };
        let n = team_rows.len().max(1);
        let group_h = n as f32 * row_h + 8.0;
        p.bevel(left, top, inner, group_h, 9.0, color.with_alpha(0.68));
        p.bevel(
            left + 1.5,
            top + 1.5,
            inner - 3.0,
            group_h - 3.0,
            8.0,
            UiColor::rgba(15, 24, 32, 255),
        );
        if team_rows.is_empty() {
            p.text(
                left + 88.0,
                top + 10.0,
                if page == 0 {
                    "No combatants"
                } else {
                    "No more combatants"
                },
                18.0,
                MUTED,
            );
        }
        for (index, row) in team_rows.iter().enumerate() {
            let ry = top + 4.0 + index as f32 * row_h;
            let highlighted = page == 0 && index == 0;
            p.fill(
                left + 5.0,
                ry,
                inner - 10.0,
                row_h,
                if highlighted {
                    if friendly {
                        UiColor::rgba(37, 63, 77, 255)
                    } else {
                        UiColor::rgba(65, 36, 44, 255)
                    }
                } else if index % 2 == 0 {
                    UiColor::rgba(24, 34, 44, 255)
                } else {
                    UiColor::rgba(19, 28, 37, 255)
                },
                0.0,
            );
            p.fill(left + 3.0, ry, 4.0, row_h, color, 0.0);
            let size = (row_h * 0.5).min(29.0);
            let text_y = ry + (row_h - size * 1.25) * 0.5;
            p.text(
                left + 24.0,
                text_y,
                (page * per_team + index + 1).to_string(),
                size,
                LIGHT,
            );
            p.tank(left + 72.0, ry + row_h * 0.27, 1.7, LIGHT);
            p.text(
                left + 145.0,
                text_y,
                clipped_name(&row.name, inner * 0.43 - 145.0, size),
                size,
                color,
            );
            if row.is_player {
                p.circle(left + inner * 0.45, ry + row_h * 0.5, 3.0, LIGHT);
            }
            for (column, value) in
                columns
                    .iter()
                    .zip([row.kills, row.deaths, row.assists, row.score])
            {
                p.centered(
                    *column,
                    text_y,
                    &grouped(value),
                    size,
                    if highlighted { LIGHT } else { MUTED },
                );
            }
        }
        top += group_h + 18.0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
