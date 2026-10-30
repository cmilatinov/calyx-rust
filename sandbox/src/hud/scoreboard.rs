//! The scoreboard: kills and deaths of every combatant, shown while the
//! scoreboard action is held.

use super::{hud_text, HudStyle};
use crate::combat::ComponentCombatStats;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{
    AlignItems, Border, CornerRadius, EdgeInsets, PointerEvents, StylePatch, UiArena, UiLength,
    UiNodeHandle, Widget,
};

/// One scoreboard row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScoreboardRow {
    pub name: String,
    pub kills: u32,
    pub deaths: u32,
    /// Whether the row is the HUD's own player.
    pub is_player: bool,
}

/// A row for every object with [`ComponentCombatStats`], most kills first,
/// then fewest deaths, then by name.
pub(crate) fn scoreboard_rows(scene: &Scene, player: GameObject) -> Vec<ScoreboardRow> {
    let mut rows: Vec<ScoreboardRow> = scene
        .objects()
        .filter_map(|object| {
            scene.read_component::<ComponentCombatStats, _, _>(object, |stats| ScoreboardRow {
                name: scene.name(object),
                kills: stats.kills,
                deaths: stats.deaths,
                is_player: object == player,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.kills
            .cmp(&a.kills)
            .then(a.deaths.cmp(&b.deaths))
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

const NUMBER_COLUMN_WIDTH: f32 = 56.0;
const ROW_HEIGHT: f32 = 26.0;
const PADDING: f32 = 14.0;
const GAP: f32 = 6.0;
const TITLE_FONT_SIZE: f32 = 16.0;

/// Height of a scoreboard of `rows` rows: title, header and divider, then
/// one line per row, or one for the empty note.
pub(super) fn scoreboard_height(rows: usize) -> f32 {
    let lines = rows.max(1);
    PADDING * 2.0
        + TITLE_FONT_SIZE * 1.25
        + ROW_HEIGHT
        + 1.0
        + lines as f32 * ROW_HEIGHT
        + (2 + lines) as f32 * GAP
}

pub(super) struct ScoreboardHud<'a> {
    pub style: HudStyle,
    pub rows: &'a [ScoreboardRow],
    pub width: f32,
    pub margin: EdgeInsets,
}

impl ScoreboardHud<'_> {
    fn row(
        &self,
        ui: &mut UiArena,
        id: &str,
        cells: [String; 3],
        font_size: f32,
        color: calyx_engine::ui::UiColor,
        background: Option<calyx_engine::ui::UiColor>,
    ) -> UiNodeHandle {
        let style = self.style;
        let [name, kills, deaths] = cells;
        let name = hud_text(
            ui,
            format!("{id}-name"),
            name,
            style.value_style(color, font_size),
        )
        .flex_grow(ui, 1.0);
        let number = |ui: &mut UiArena, column: &str, value: String| {
            hud_text(
                ui,
                format!("{id}-{column}"),
                value,
                style.value_style(color, font_size),
            )
            .width(ui, UiLength::Px(NUMBER_COLUMN_WIDTH))
        };
        let kills = number(ui, "kills", kills);
        let deaths = number(ui, "deaths", deaths);
        let mut patch = StylePatch::default()
            .padding(EdgeInsets::symmetric(8.0, 4.0))
            .align_items(AlignItems::Center)
            .radius(CornerRadius::all(2.0))
            .pointer_events(PointerEvents::None);
        if let Some(background) = background {
            patch = patch.background(background);
        }
        ui.row()
            .id(ui, id.to_string())
            .style(ui, patch)
            .width(ui, UiLength::Fill)
            .height(ui, UiLength::Px(ROW_HEIGHT))
            .children(ui, [name, kills, deaths])
    }
}

impl Widget for ScoreboardHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let style = self.style;
        let title = hud_text(
            ui,
            "hud-scoreboard-title",
            "SCOREBOARD",
            style.value_style(style.text, TITLE_FONT_SIZE),
        );
        let header = self.row(
            ui,
            "hud-scoreboard-header",
            ["COMBATANT".into(), "KILLS".into(), "DEATHS".into()],
            11.0,
            style.muted,
            None,
        );
        let divider = ui
            .container()
            .id(ui, "hud-scoreboard-divider")
            .background(ui, style.outline)
            .width(ui, UiLength::Fill)
            .height(ui, UiLength::Px(1.0))
            .pointer_events(ui, PointerEvents::None);
        let mut children = vec![title, header, divider];
        for (index, row) in self.rows.iter().enumerate() {
            let (color, background) = if row.is_player {
                (style.ammo, Some(style.bar_background))
            } else {
                (style.text, None)
            };
            children.push(self.row(
                ui,
                &format!("hud-scoreboard-row-{index}"),
                [
                    row.name.to_uppercase(),
                    row.kills.to_string(),
                    row.deaths.to_string(),
                ],
                14.0,
                color,
                background,
            ));
        }
        if self.rows.is_empty() {
            children.push(hud_text(
                ui,
                "hud-scoreboard-empty",
                "NO COMBATANTS",
                style.label_style(style.muted),
            ));
        }

        ui.column()
            .id(ui, "hud-scoreboard")
            .style(
                ui,
                style
                    .panel_style()
                    .background(style.panel_strong)
                    .border(Border::solid(style.outline, 1.0))
                    .padding(EdgeInsets::all(PADDING))
                    .gap(GAP),
            )
            .width(ui, UiLength::Px(self.width))
            .height(ui, UiLength::Px(scoreboard_height(self.rows.len())))
            .margin(ui, self.margin)
            .children(ui, children)
    }
}

#[cfg(test)]
mod tests {
    use super::{scoreboard_rows, ScoreboardRow};
    use crate::combat::ComponentCombatStats;
    use calyx_engine::component::ComponentID;

    #[test]
    fn scoreboard_rows_rank_combatants_by_kills_then_deaths() {
        let mut scene = calyx_engine::test_support::test_scene();
        let mut add = |name: &str, kills, deaths| {
            let object = scene.create(
                Some(ComponentID {
                    name: name.to_string(),
                    ..Default::default()
                }),
                None,
            );
            scene.add_component(object, ComponentCombatStats { kills, deaths });
            object
        };
        let tank = add("Tank", 2, 1);
        add("Sentry", 2, 0);
        add("Rookie", 0, 3);
        scene.create(None, None);

        let rows = scoreboard_rows(&scene, tank);

        let row = |name: &str, kills, deaths, is_player| ScoreboardRow {
            name: name.to_string(),
            kills,
            deaths,
            is_player,
        };
        assert_eq!(
            rows,
            vec![
                row("Sentry", 2, 0, false),
                row("Tank", 2, 1, true),
                row("Rookie", 0, 3, false),
            ]
        );
    }
}
