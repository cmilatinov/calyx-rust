//! Confirmed kills: team colours for tanks, neutral attribution for hazards.
use super::paint::{alpha, Painter, BLACK, CYAN, LIGHT, MUTED, RED, STEEL};
use crate::combat::{combat_team, ComponentKillFeed};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::UiColor;

pub(super) fn paint_feed(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let team = combat_team(scene, player);
    let _ = scene.read_component::<ComponentKillFeed, _, _>(player, |feed| {
        for (index, entry) in feed.entries().take(2).enumerate() {
            let opacity = feed.opacity(entry);
            let y = y + index as f32 * (h + 7.0);
            let start = p.commands.len();
            let color = |other| match other {
                None => MUTED,
                Some(t) if Some(t) == team => CYAN,
                _ => RED,
            };
            let kc = color(entry.killer_team);
            let vc = color(entry.victim_team);
            let slant = h * 0.23;
            for (inset, c) in [
                (0.0, BLACK),
                (2.5, STEEL),
                (5.0, UiColor::rgba(10, 19, 28, 225)),
            ] {
                p.polygon(
                    &[
                        (x + slant + inset, y + inset),
                        (x + w - inset, y + inset),
                        (x + w - slant - inset, y + h - inset),
                        (x + inset, y + h - inset),
                    ],
                    c,
                );
            }
            p.polygon(
                &[
                    (x + slant, y + 2.0),
                    (x + slant + 12.0, y + 2.0),
                    (x + 14.0, y + h - 2.0),
                    (x + 2.0, y + h - 2.0),
                ],
                kc,
            );
            p.polygon(
                &[
                    (x + w - 14.0, y + 2.0),
                    (x + w - 2.0, y + 2.0),
                    (x + w - slant, y + h - 2.0),
                    (x + w - slant - 12.0, y + h - 2.0),
                ],
                vc,
            );
            let icon_scale = (h * 0.55) / 24.0;
            let center = x + w * 0.5;
            p.tank(center - 29.0 * icon_scale, y + h * 0.23, icon_scale, LIGHT);
            p.polygon(
                &[
                    (center + 27.0 * icon_scale, y + h * 0.38),
                    (center + 33.0 * icon_scale, y + h * 0.5),
                    (center + 27.0 * icon_scale, y + h * 0.62),
                ],
                LIGHT,
            );
            p.middle(
                x + w * 0.23,
                y + h * 0.5,
                entry.killer.as_deref().unwrap_or("Environment"),
                h * 0.58,
                kc,
                Some(w * 0.32),
            );
            p.middle(
                x + w * 0.78,
                y + h * 0.5,
                &entry.victim,
                h * 0.58,
                vc,
                Some(w * 0.32),
            );
            for command in &mut p.commands[start..] {
                use calyx_engine::ui::PaintCommand::*;
                match command {
                    FillRect { color, .. }
                    | FillPolygon { color, .. }
                    | Text { color, .. }
                    | Line { color, .. } => *color = alpha(*color, opacity),
                    Label(label) => {
                        label.color = alpha(label.color, opacity);
                        if let Some(outline) = &mut label.outline {
                            outline.color = alpha(outline.color, opacity);
                        }
                    }
                    ColorMesh { colors, .. } => {
                        for c in colors {
                            *c = alpha(*c, opacity)
                        }
                    }
                    _ => {}
                }
            }
        }
    });
}
