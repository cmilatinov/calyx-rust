//! The kill feed: the latest kills, newest at the top, fading as they age.

use super::{hud_text, HudStyle};
use crate::combat::ComponentKillFeed;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{
    AlignItems, Border, CornerRadius, EdgeInsets, PointerEvents, StylePatch, UiArena, UiLength,
    UiNodeHandle, Widget,
};

/// One kill feed line as the HUD draws it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KillFeedLine {
    pub killer: Option<String>,
    pub victim: String,
    pub opacity: f32,
}

/// The lines of the kill feed on `game_object`, newest first; none when it
/// has no feed.
pub(crate) fn kill_feed_lines(scene: &Scene, game_object: GameObject) -> Vec<KillFeedLine> {
    scene
        .read_component::<ComponentKillFeed, _, _>(game_object, |feed| {
            feed.entries()
                .map(|entry| KillFeedLine {
                    killer: entry.killer.clone(),
                    victim: entry.victim.clone(),
                    opacity: feed.opacity(entry),
                })
                .filter(|line| line.opacity > 0.0)
                .collect()
        })
        .unwrap_or_default()
}

const FONT_SIZE: f32 = 13.0;
const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 5.0;
const GAP: f32 = 5.0;

/// A width that fits `text` at `font_size` in the HUD's proportional font.
/// Layout cannot measure glyphs, and its own estimate of half the font size
/// per character runs short of capitals, so the parts of a line would
/// overlap.
fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                0.3
            } else if character.is_lowercase() {
                0.56
            } else {
                0.7
            }
        })
        .sum::<f32>()
        * font_size
}

pub(super) struct KillFeedHud<'a> {
    pub style: HudStyle,
    pub lines: &'a [KillFeedLine],
    pub width: f32,
    pub margin: EdgeInsets,
}

impl Widget for KillFeedHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let style = self.style;
        let rows: Vec<UiNodeHandle> = self
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let fade = |color: calyx_engine::ui::UiColor| color.with_alpha(line.opacity);
                let mut width = PADDING_X * 2.0;
                let mut text = |ui: &mut UiArena, part: &str, value: &str, color| {
                    let part_width = estimated_text_width(value, FONT_SIZE);
                    width += part_width;
                    hud_text(
                        ui,
                        format!("hud-kill-feed-{index}-{part}"),
                        value,
                        style.value_style(fade(color), FONT_SIZE),
                    )
                    .width(ui, UiLength::Px(part_width))
                };
                // "<killer> destroyed <victim>", or "<victim> was destroyed"
                // when nobody gets the kill.
                let parts = match &line.killer {
                    Some(killer) => vec![
                        text(ui, "killer", &killer.to_uppercase(), style.ammo),
                        text(ui, "verb", "destroyed", style.muted),
                        text(ui, "victim", &line.victim.to_uppercase(), style.text),
                    ],
                    None => vec![
                        text(ui, "victim", &line.victim.to_uppercase(), style.text),
                        text(ui, "verb", "was destroyed", style.muted),
                    ],
                };
                width += GAP * (parts.len() - 1) as f32;
                ui.row()
                    .id(ui, format!("hud-kill-feed-{index}"))
                    .style(
                        ui,
                        StylePatch::default()
                            .background(fade(style.panel))
                            .border(Border::solid(fade(style.outline), 1.0))
                            .radius(CornerRadius::all(3.0))
                            .padding(EdgeInsets::symmetric(PADDING_X, PADDING_Y))
                            .gap(GAP)
                            .pointer_events(PointerEvents::None),
                    )
                    .width(ui, UiLength::Px(width.min(self.width)))
                    .height(ui, UiLength::Px(FONT_SIZE * 1.25 + PADDING_Y * 2.0))
                    .children(ui, parts)
            })
            .collect();
        ui.column()
            .id(ui, "hud-kill-feed")
            .gap(ui, 4.0)
            .align_items(ui, AlignItems::End)
            .width(ui, UiLength::Px(self.width))
            .margin(ui, self.margin)
            .pointer_events(ui, PointerEvents::None)
            .children(ui, rows)
    }
}
