//! The screens around a match: the main menu, the lobby, and the end
//! screen's buttons under the final scoreboard. They follow the scene's Game
//! State (`crate::game_state`); a scene without one goes straight to play.
//!
//! Layout is a pure function of the screen and the view size, so the HUD's
//! update hit-tests the same buttons its paint draws.
use super::paint::{Painter, CYAN, LIGHT, MUTED, PANEL, RED, STEEL};
use crate::combat::{is_tank_combatant, same_side};
use crate::game_state::{game_phase, ComponentGameState, GamePhase};
use crate::match_rules::{match_rules, MatchMode};
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{TextAnchor, UiColor};

/// What a button does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    /// Main menu: enter the lobby.
    Play,
    /// Main menu: leave play.
    Quit,
    /// Lobby: the local player is ready, which starts the match.
    Ready,
    /// Lobby and end screen: back to the main menu.
    MainMenu,
    /// End screen: back to the lobby for another match.
    Rematch,
}

/// The screen the HUD shows over the arena, outside play.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuScreen {
    MainMenu,
    Lobby,
    /// The final scoreboard and its buttons.
    GameOver,
}

impl MenuScreen {
    /// The screen of the scene's Game State, or `None` while it is playing or
    /// when the scene has none.
    pub fn in_scene(scene: &Scene) -> Option<Self> {
        match game_phase(scene)? {
            GamePhase::MainMenu => Some(Self::MainMenu),
            GamePhase::Lobby => Some(Self::Lobby),
            GamePhase::GameOver => Some(Self::GameOver),
            GamePhase::Playing => None,
        }
    }
}

/// One button, in HUD points from the view's top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuButton {
    pub action: MenuAction,
    pub label: &'static str,
    /// The screen's main choice, which Enter also picks.
    pub primary: bool,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl MenuButton {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// The interface scale for a view `height` points tall, shared with the
/// scoreboard.
pub(super) fn unit(height: f32) -> f32 {
    (height / 941.0).clamp(0.65, 1.2)
}

fn button_size(height: f32) -> (f32, f32) {
    let u = unit(height);
    (330.0 * u, 64.0 * u)
}

/// The height the end screen's buttons take from the bottom of the view; the
/// final scoreboard centres above it.
pub(super) fn game_over_reserve(height: f32) -> f32 {
    let (_, h) = button_size(height);
    h + 2.0 * 26.0 * unit(height)
}

struct Panel {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn main_menu_panel(width: f32, height: f32) -> Panel {
    let u = unit(height);
    let (bw, bh) = button_size(height);
    let w = (bw + 2.0 * 60.0 * u).min(width - 36.0);
    let h = 150.0 * u + 2.0 * bh + 20.0 * u + 56.0 * u;
    Panel {
        x: (width - w) * 0.5,
        y: (height - h) * 0.5 + 20.0 * u,
        w,
        h,
    }
}

fn lobby_panel(width: f32, height: f32) -> Panel {
    let u = unit(height);
    let w = (width * 0.62)
        .clamp(640.0 * u, 1180.0 * u)
        .min(width - 36.0);
    let h = (height * 0.72).max(420.0 * u).min(height - 60.0 * u);
    Panel {
        x: (width - w) * 0.5,
        y: (height - h) * 0.5 + 20.0 * u,
        w,
        h,
    }
}

/// The buttons `screen` shows in a `width` by `height` view.
pub fn menu_buttons(screen: MenuScreen, width: f32, height: f32) -> Vec<MenuButton> {
    let u = unit(height);
    let (bw, bh) = button_size(height);
    let button = |action, label, primary, x, y| MenuButton {
        action,
        label,
        primary,
        x,
        y,
        w: bw,
        h: bh,
    };
    // Two buttons side by side, centred: the way back on the left, the way
    // on to the right.
    let pair = |y: f32, back: (MenuAction, &'static str), on: (MenuAction, &'static str)| {
        let gap = 28.0 * u;
        let w = bw.min((width - 36.0 - gap) * 0.5);
        let center = width * 0.5;
        vec![
            MenuButton {
                w,
                ..button(back.0, back.1, false, center - gap * 0.5 - w, y)
            },
            MenuButton {
                w,
                ..button(on.0, on.1, true, center + gap * 0.5, y)
            },
        ]
    };
    match screen {
        MenuScreen::MainMenu => {
            let panel = main_menu_panel(width, height);
            let x = (width - bw) * 0.5;
            let top = panel.y + 150.0 * u;
            vec![
                button(MenuAction::Play, "PLAY", true, x, top),
                button(MenuAction::Quit, "QUIT", false, x, top + bh + 20.0 * u),
            ]
        }
        MenuScreen::Lobby => {
            let panel = lobby_panel(width, height);
            pair(
                panel.y + panel.h - bh - 36.0 * u,
                (MenuAction::MainMenu, "MAIN MENU"),
                (MenuAction::Ready, "READY"),
            )
        }
        MenuScreen::GameOver => pair(
            height - bh - 26.0 * u,
            (MenuAction::MainMenu, "MAIN MENU"),
            (MenuAction::Rematch, "REMATCH"),
        ),
    }
}

/// How the pointer is over a screen's buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct MenuPointer {
    pub hovered: Option<MenuAction>,
    /// The button the pointer went down on, until it comes up.
    pub pressed: Option<MenuAction>,
}

/// "TEAM DEATHMATCH  /  20 KILLS  /  05:00" for the scene's Match Rules.
fn rules_line(scene: &Scene) -> Option<String> {
    let rules = match_rules(scene)?;
    let mut parts = vec![match rules.mode() {
        MatchMode::FreeForAll => "FREE FOR ALL".to_string(),
        MatchMode::TeamDeathmatch => "TEAM DEATHMATCH".to_string(),
    }];
    if rules.kill_limit > 0 {
        parts.push(format!("{} KILLS", rules.kill_limit));
    }
    if rules.time_limit > 0.0 {
        parts.push(super::format_time(rules.time_limit));
    }
    Some(parts.join("  /  "))
}

pub(super) fn paint_buttons(p: &mut Painter, buttons: &[MenuButton], pointer: MenuPointer) {
    for button in buttons {
        let u = button.h / 64.0;
        let hovered = pointer.hovered == Some(button.action);
        let pressed = hovered && pointer.pressed == Some(button.action);
        let edge = match (button.primary, hovered) {
            (true, _) => CYAN,
            (false, true) => UiColor::rgba(150, 166, 180, 255),
            (false, false) => STEEL,
        };
        let fill = match (button.primary, hovered, pressed) {
            (_, _, true) => UiColor::rgba(10, 16, 22, 250),
            (true, true, _) => UiColor::rgba(25, 64, 81, 255),
            (true, false, _) => UiColor::rgba(17, 38, 50, 250),
            (false, true, _) => UiColor::rgba(36, 48, 60, 250),
            (false, false, _) => PANEL,
        };
        let cut = 16.0 * u;
        p.bevel(button.x, button.y, button.w, button.h, cut, edge);
        p.bevel(
            button.x + 3.0,
            button.y + 3.0,
            button.w - 6.0,
            button.h - 6.0,
            (cut - 2.0).max(1.0),
            fill,
        );
        let (cx, cy) = button.center();
        p.middle(
            cx,
            cy,
            button.label,
            28.0 * u,
            if button.primary || hovered {
                LIGHT
            } else {
                MUTED
            },
            Some(button.w - 24.0 * u),
        );
    }
}

/// A bevelled panel with its title plate across the top edge, as the
/// scoreboard's.
fn titled_panel(p: &mut Painter, panel: &Panel, title: &str, u: f32) {
    p.panel(panel.x, panel.y, panel.w, panel.h, 30.0 * u);
    let title_w = (panel.w * 0.5).max(300.0 * u).min(panel.w - 40.0 * u);
    let title_h = 86.0 * u;
    let title_x = panel.x + (panel.w - title_w) * 0.5;
    let title_y = panel.y - title_h * 0.48;
    p.title_plate(title_x, title_y, title_w, title_h);
    p.middle(
        panel.x + panel.w * 0.5,
        title_y + title_h * 0.5,
        title,
        44.0 * u,
        LIGHT,
        Some(title_w - 40.0 * u),
    );
}

pub(super) fn paint_main_menu(
    p: &mut Painter,
    scene: &Scene,
    width: f32,
    height: f32,
    pointer: MenuPointer,
) {
    let u = unit(height);
    p.backdrop(width, height);
    let panel = main_menu_panel(width, height);
    titled_panel(p, &panel, "TANK ARENA", u);
    if let Some(line) = rules_line(scene) {
        p.middle(
            width * 0.5,
            panel.y + 82.0 * u,
            line,
            20.0 * u,
            MUTED,
            Some(panel.w - 40.0 * u),
        );
    }
    paint_buttons(
        p,
        &menu_buttons(MenuScreen::MainMenu, width, height),
        pointer,
    );
}

/// One tank in the lobby's roster.
struct Entrant {
    name: String,
    friendly: bool,
    is_player: bool,
}

fn roster(scene: &Scene, player: GameObject) -> Vec<Entrant> {
    let mut entrants: Vec<_> = scene
        .objects()
        .filter(|&object| is_tank_combatant(scene, object))
        .map(|object| Entrant {
            name: scene.name(object),
            friendly: object == player || same_side(scene, player, object),
            is_player: object == player,
        })
        .collect();
    entrants.sort_by(|a, b| {
        b.is_player
            .cmp(&a.is_player)
            .then(b.friendly.cmp(&a.friendly))
            .then(a.name.cmp(&b.name))
    });
    entrants
}

pub(super) fn paint_lobby(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    width: f32,
    height: f32,
    pointer: MenuPointer,
) {
    let u = unit(height);
    p.backdrop(width, height);
    let panel = lobby_panel(width, height);
    titled_panel(p, &panel, "LOBBY", u);
    if let Some(line) = rules_line(scene) {
        p.middle(
            width * 0.5,
            panel.y + 82.0 * u,
            line,
            20.0 * u,
            MUTED,
            Some(panel.w - 40.0 * u),
        );
    }
    let ready = scene
        .objects()
        .find_map(|object| {
            scene.read_component::<ComponentGameState, _, _>(object, |state| state.players_ready)
        })
        .unwrap_or(false);
    let entrants = roster(scene, player);
    let teams = crate::match_rules::match_mode(scene) == MatchMode::TeamDeathmatch;
    let (_, bh) = button_size(height);
    let padding = 36.0 * u;
    let top = panel.y + 120.0 * u;
    let bottom = panel.y + panel.h - bh - 36.0 * u - 28.0 * u;
    let inner = panel.w - padding * 2.0;
    let gap = 28.0 * u;
    // Team deathmatch shows each side in its own column; free for all lists
    // every tank in one, each in its side's colour.
    let columns: Vec<(Option<bool>, f32, f32)> = if teams {
        let w = (inner - gap) * 0.5;
        vec![
            (Some(true), panel.x + padding, w),
            (Some(false), panel.x + padding + w + gap, w),
        ]
    } else {
        vec![(None, panel.x + padding, inner)]
    };
    let row_h = 46.0 * u;
    let font = 22.0 * u;
    for (side, x, w) in columns {
        let rows: Vec<_> = entrants
            .iter()
            .filter(|entrant| side.is_none_or(|friendly| entrant.friendly == friendly))
            .collect();
        let color = if side == Some(false) { RED } else { CYAN };
        let heading = match side {
            Some(true) => "FRIENDLY",
            Some(false) => "OPPOSING",
            None => "PLAYERS",
        };
        p.label(
            x + 18.0 * u,
            top + 16.0 * u,
            heading,
            18.0 * u,
            color,
            TextAnchor::CenterLeft,
            None,
            false,
            None,
        );
        let list_top = top + 36.0 * u;
        let capacity = (((bottom - list_top) / row_h).floor() as usize).max(1);
        let shown = rows.len().clamp(1, capacity);
        let list_h = shown as f32 * row_h;
        p.bevel(
            x,
            list_top,
            w,
            list_h,
            14.0 * u,
            UiColor::rgba(77, 95, 108, 220),
        );
        p.bevel(
            x + 2.0,
            list_top + 2.0,
            w - 4.0,
            list_h - 4.0,
            13.0 * u,
            UiColor::rgba(14, 23, 30, 250),
        );
        p.fill(
            x + 2.0,
            list_top + 14.0 * u,
            4.0 * u,
            list_h - 28.0 * u,
            color,
            0.0,
        );
        if rows.is_empty() {
            p.label(
                x + 28.0 * u,
                list_top + row_h * 0.5,
                "No combatants",
                font,
                MUTED,
                TextAnchor::CenterLeft,
                None,
                false,
                None,
            );
        }
        let overflow = rows.len() > capacity;
        for (index, entrant) in rows.iter().take(capacity).enumerate() {
            let cy = list_top + index as f32 * row_h + row_h * 0.5;
            if overflow && index + 1 == capacity {
                p.label(
                    x + 28.0 * u,
                    cy,
                    format!("+{} MORE", rows.len() - capacity + 1),
                    font,
                    MUTED,
                    TextAnchor::CenterLeft,
                    None,
                    false,
                    None,
                );
                break;
            }
            if index % 2 == 1 {
                p.fill(
                    x + 8.0 * u,
                    cy - row_h * 0.5,
                    w - 12.0 * u,
                    row_h,
                    UiColor::rgba(25, 35, 44, 255),
                    0.0,
                );
            }
            let name_color = if entrant.friendly { CYAN } else { RED };
            if entrant.is_player {
                p.polygon(
                    &[
                        (x + 16.0 * u, cy - 6.0 * u),
                        (x + 23.0 * u, cy),
                        (x + 16.0 * u, cy + 6.0 * u),
                    ],
                    name_color,
                );
            }
            let ts = (row_h * 0.4) / 24.0;
            p.tank(x + 32.0 * u, cy - 12.0 * ts, ts, LIGHT);
            let status_x = x + w - 20.0 * u;
            p.label(
                x + 32.0 * u + 58.0 * ts,
                cy,
                &entrant.name,
                font * 1.05,
                name_color,
                TextAnchor::CenterLeft,
                Some(w - 58.0 * ts - 190.0 * u),
                false,
                None,
            );
            // Bots are always ready; the local player readies with the button.
            let entrant_ready = !entrant.is_player || ready;
            p.label(
                status_x,
                cy,
                if entrant_ready { "READY" } else { "NOT READY" },
                18.0 * u,
                if entrant_ready { LIGHT } else { MUTED },
                TextAnchor::CenterRight,
                None,
                false,
                None,
            );
        }
    }
    paint_buttons(p, &menu_buttons(MenuScreen::Lobby, width, height), pointer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_fit_the_view_and_do_not_overlap() {
        for (width, height) in [
            (1280.0, 720.0),
            (960.0, 540.0),
            (2560.0, 1440.0),
            (640.0, 480.0),
        ] {
            for screen in [
                MenuScreen::MainMenu,
                MenuScreen::Lobby,
                MenuScreen::GameOver,
            ] {
                let buttons = menu_buttons(screen, width, height);
                assert_eq!(buttons.len(), 2);
                assert_eq!(buttons.iter().filter(|b| b.primary).count(), 1);
                for b in &buttons {
                    assert!(b.x >= 0.0 && b.y >= 0.0, "{screen:?} {b:?}");
                    assert!(
                        b.x + b.w <= width && b.y + b.h <= height,
                        "{screen:?} {b:?}"
                    );
                }
                let (a, b) = (buttons[0], buttons[1]);
                let (cx, cy) = b.center();
                assert!(!a.contains(cx, cy), "{screen:?} buttons overlap");
                assert!(b.contains(cx, cy));
            }
        }
    }
}
