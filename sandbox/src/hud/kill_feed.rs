//! Recent confirmed kills, with team colours captured at the time of the kill.
use super::paint::{alpha, clipped_name, Painter, CYAN, LIGHT, MUTED, RED};
use crate::combat::{combat_team, ComponentKillFeed};
use calyx_engine::scene::{GameObject, Scene};

pub(super) fn paint_feed(p: &mut Painter, scene: &Scene, player: GameObject, x: f32, y: f32) {
    let team = combat_team(scene, player);
    let _ = scene.read_component::<ComponentKillFeed, _, _>(player, |feed| {
        for (index, entry) in feed.entries().take(2).enumerate() {
            let opacity = feed.opacity(entry);
            let y = y + index as f32 * 59.0;
            let start = p.commands.len();
            p.panel(x, y, 475.0, 50.0, 9.0);
            let color = |other| {
                if team.is_some() && other == team {
                    CYAN
                } else {
                    RED
                }
            };
            let killer_color = entry.killer_team.map_or(CYAN, |t| color(Some(t)));
            p.bevel(x + 3.0, y + 3.0, 5.0, 44.0, 2.0, killer_color);
            p.bevel(x + 467.0, y + 3.0, 5.0, 44.0, 2.0, color(entry.victim_team));
            p.text(
                x + 18.0,
                y + 10.0,
                clipped_name(
                    entry.killer.as_deref().unwrap_or("Environment"),
                    180.0,
                    23.0,
                ),
                23.0,
                if entry.killer.is_some() {
                    killer_color
                } else {
                    MUTED
                },
            );
            p.tank(x + 208.0, y + 12.0, 1.5, LIGHT);
            p.polygon(
                &[
                    (x + 266.0, y + 21.0),
                    (x + 273.0, y + 25.0),
                    (x + 266.0, y + 29.0),
                ],
                LIGHT,
            );
            p.text(
                x + 288.0,
                y + 10.0,
                clipped_name(&entry.victim, 164.0, 23.0),
                23.0,
                color(entry.victim_team),
            );
            for command in &mut p.commands[start..] {
                use calyx_engine::ui::PaintCommand::*;
                match command {
                    FillRect { color, .. }
                    | FillPolygon { color, .. }
                    | Text { color, .. }
                    | Line { color, .. } => *color = alpha(*color, opacity),
                    _ => {}
                }
            }
        }
    });
}
