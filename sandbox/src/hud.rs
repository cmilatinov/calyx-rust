mod kill_feed;
mod minimap;
mod scoreboard;

use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentEventContext, ComponentHud, ComponentUpdate, ReflectComponent,
    ReflectComponentHud, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{
    Border, CornerRadius, EdgeInsets, JustifyContent, PointerEvents, StylePatch, UiArena, UiColor,
    UiLength, UiNodeHandle, UiRect, UiSize, Widget,
};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use kill_feed::{kill_feed_lines, KillFeedHud, KillFeedLine};
use minimap::{minimap_state, MinimapHud, MinimapState};
use scoreboard::{scoreboard_height, scoreboard_rows, ScoreboardHud, ScoreboardRow};
use serde::{Deserialize, Serialize};

/// Draws the HUD of the tank on the same game object: health, weapon, a
/// minimap, the kill feed of a [`ComponentKillFeed`](crate::combat::ComponentKillFeed)
/// on the same object, and, while the `scoreboard` action is held, the
/// scoreboard.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b7f3a8d2-5c41-4e8b-9a6f-0d2e7c1b4f93"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentHud)]
#[reflect_attr(name = "Tank HUD")]
#[serde(default)]
pub struct ComponentTankHud {
    pub screen_margin: f32,
    /// Whether the scoreboard action was held on the last update.
    #[serde(skip)]
    #[reflect_skip]
    pub scoreboard_open: bool,
}

impl Default for ComponentTankHud {
    fn default() -> Self {
        Self {
            screen_margin: HudStyle::default().screen_margin,
            scoreboard_open: false,
        }
    }
}

impl Component for ComponentTankHud {}

impl ComponentUpdate for ComponentTankHud {
    fn update(
        &self,
        ComponentEventContext {
            scene, game_object, ..
        }: ComponentEventContext,
        _resources: &mut ResourceMap,
        input: &Input,
    ) {
        let open = input.action("scoreboard").pressed();
        let _ = scene.write_component::<ComponentTankHud, _>(game_object, |hud| {
            hud.scoreboard_open = open;
        });
    }
}

impl ComponentHud for ComponentTankHud {
    fn build_hud(
        &self,
        scene: &Scene,
        game_object: GameObject,
        ui: &mut UiArena,
        viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        let hud = scene.read_component::<ComponentTankHud, _, _>(game_object, |hud| *hud)?;
        let state = tank_hud_state(scene, game_object)?;
        Some(
            SandboxHud {
                viewport,
                style: HudStyle {
                    screen_margin: hud.screen_margin,
                    ..Default::default()
                },
                health: state.health,
                max_health: state.max_health,
                weapon_name: "Cannon",
                ammo_loaded: state.ammo,
                ammo_capacity: state.max_ammo,
                reload_remaining: state.reload_remaining,
                fire_cooldown_remaining: state.fire_cooldown_remaining,
                gameplay_locked: state.gameplay_locked,
                respawn_remaining: state.respawn_remaining,
                minimap: minimap_state(scene, game_object),
                kill_feed: kill_feed_lines(scene, game_object),
                scoreboard: hud
                    .scoreboard_open
                    .then(|| scoreboard_rows(scene, game_object)),
            }
            .build(ui),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct TankHudState {
    health: f32,
    max_health: f32,
    ammo: u32,
    max_ammo: u32,
    reload_remaining: f32,
    fire_cooldown_remaining: f32,
    gameplay_locked: bool,
    /// Seconds until the tank respawns, while it is dead.
    respawn_remaining: Option<f32>,
}

fn tank_hud_state(scene: &Scene, game_object: GameObject) -> Option<TankHudState> {
    let controller = scene
        .read_component::<ComponentTankController, _, _>(game_object, |component| *component)?;
    let health =
        scene.read_component::<ComponentHealth, _, _>(game_object, |component| *component)?;
    let respawn =
        scene.read_component::<ComponentRespawnState, _, _>(game_object, |component| *component);
    Some(TankHudState {
        health: health.current_health,
        max_health: health.max_health,
        ammo: controller.ammo,
        max_ammo: controller.max_ammo,
        reload_remaining: controller.reload_remaining,
        fire_cooldown_remaining: controller.fire_cooldown_remaining,
        gameplay_locked: respawn.is_some_and(|respawn| respawn.is_gameplay_locked()),
        respawn_remaining: respawn
            .filter(|respawn| !respawn.alive)
            .map(|respawn| respawn.respawn_remaining),
    })
}

#[derive(Clone, Copy)]
struct HudStyle {
    screen_margin: f32,
    panel: UiColor,
    panel_strong: UiColor,
    outline: UiColor,
    text: UiColor,
    muted: UiColor,
    health: UiColor,
    ammo: UiColor,
    bar_background: UiColor,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            screen_margin: 28.0,
            panel: UiColor::rgba(10, 12, 14, 205),
            panel_strong: UiColor::rgba(8, 10, 12, 235),
            outline: UiColor::rgba(132, 142, 146, 185),
            text: UiColor::rgba(244, 244, 238, 245),
            muted: UiColor::rgba(170, 174, 170, 210),
            health: UiColor::rgba(92, 205, 110, 245),
            ammo: UiColor::rgba(238, 217, 132, 245),
            bar_background: UiColor::rgba(34, 36, 38, 220),
        }
    }
}

impl HudStyle {
    fn margin(self, viewport: UiRect) -> f32 {
        self.screen_margin.min(viewport.width() * 0.04).max(18.0)
    }

    fn panel_style(self) -> StylePatch {
        StylePatch::default()
            .background(self.panel)
            .border(Border::solid(self.outline, 1.0))
            .radius(CornerRadius::all(3.0))
            .padding(EdgeInsets::all(10.0))
            .gap(7.0)
            .pointer_events(PointerEvents::None)
    }

    fn label_style(self, color: UiColor) -> StylePatch {
        StylePatch::default()
            .text_color(color)
            .font_size(11.0)
            .pointer_events(PointerEvents::None)
    }

    fn value_style(self, color: UiColor, size: f32) -> StylePatch {
        StylePatch::default()
            .text_color(color)
            .font_size(size)
            .pointer_events(PointerEvents::None)
    }
}

struct SandboxHud<'a> {
    viewport: UiRect,
    style: HudStyle,
    health: f32,
    max_health: f32,
    weapon_name: &'a str,
    ammo_loaded: u32,
    ammo_capacity: u32,
    reload_remaining: f32,
    fire_cooldown_remaining: f32,
    gameplay_locked: bool,
    respawn_remaining: Option<f32>,
    minimap: MinimapState,
    kill_feed: Vec<KillFeedLine>,
    /// The scoreboard's rows while it is open.
    scoreboard: Option<Vec<ScoreboardRow>>,
}

impl Widget for SandboxHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let margin = self.style.margin(self.viewport);
        let health_size = UiSize::new((self.viewport.width() * 0.28).clamp(250.0, 340.0), 76.0);
        let ammo_size = UiSize::new((self.viewport.width() * 0.22).clamp(220.0, 300.0), 114.0);

        let health = HealthHud {
            id: "hud-health",
            style: self.style,
            current: self.health,
            max: self.max_health,
            size: health_size,
            margin: EdgeInsets {
                top: self.viewport.height() - health_size.height - margin,
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);
        let ammo = AmmoHud {
            id: "hud-ammo",
            style: self.style,
            weapon_name: self.weapon_name,
            loaded: self.ammo_loaded,
            capacity: self.ammo_capacity,
            reload_remaining: self.reload_remaining,
            fire_cooldown_remaining: self.fire_cooldown_remaining,
            gameplay_locked: self.gameplay_locked,
            respawn_remaining: self.respawn_remaining,
            size: ammo_size,
            margin: EdgeInsets {
                top: self.viewport.height() - ammo_size.height - margin,
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - ammo_size.width - margin).max(margin),
            },
        }
        .build(ui);

        let minimap_side = (self.viewport.width() * 0.17).clamp(170.0, 230.0);
        let minimap = MinimapHud {
            style: self.style,
            state: self.minimap.clone(),
            // The label sits over a square map.
            size: UiSize::new(minimap_side, minimap_side + 24.0),
            margin: EdgeInsets {
                top: margin,
                right: 0.0,
                bottom: 0.0,
                left: margin,
            },
        }
        .build(ui);

        let kill_feed_width = (self.viewport.width() * 0.26).clamp(240.0, 340.0);
        let kill_feed = KillFeedHud {
            style: self.style,
            lines: &self.kill_feed,
            width: kill_feed_width,
            margin: EdgeInsets {
                top: margin,
                right: 0.0,
                bottom: 0.0,
                left: (self.viewport.width() - kill_feed_width - margin).max(margin),
            },
        }
        .build(ui);

        let mut layers = vec![health, ammo, minimap, kill_feed];
        if let Some(rows) = &self.scoreboard {
            let width = (self.viewport.width() * 0.4).clamp(320.0, 460.0);
            let height = scoreboard_height(rows.len());
            let scoreboard = ScoreboardHud {
                style: self.style,
                rows,
                width,
                margin: EdgeInsets {
                    top: ((self.viewport.height() - height) * 0.35).max(margin),
                    right: 0.0,
                    bottom: 0.0,
                    left: ((self.viewport.width() - width) * 0.5).max(margin),
                },
            };
            layers.push(scoreboard.build(ui));
        }

        ui.stack()
            .id(ui, "sandbox-ui")
            .width(ui, UiLength::Px(self.viewport.width()))
            .height(ui, UiLength::Px(self.viewport.height()))
            .pointer_events(ui, PointerEvents::None)
            .children(ui, layers)
    }
}

struct HealthHud {
    id: &'static str,
    style: HudStyle,
    current: f32,
    max: f32,
    size: UiSize,
    margin: EdgeInsets,
}

impl Widget for HealthHud {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let ratio = if self.max > 0.0 {
            self.current / self.max
        } else {
            0.0
        }
        .clamp(0.0, 1.0);
        let label = hud_text(
            ui,
            "hud-health-label",
            "HULL INTEGRITY",
            self.style.label_style(self.style.muted),
        );
        let value = hud_text(
            ui,
            "hud-health-value",
            format!(
                "{:03}/{:03}",
                self.current.max(0.0) as u32,
                self.max.max(0.0) as u32
            ),
            self.style.value_style(self.style.text, 16.0),
        );
        let header = ui
            .row()
            .id(ui, "hud-health-header")
            .justify_content(ui, JustifyContent::SpaceBetween)
            .pointer_events(ui, PointerEvents::None)
            .child(ui, label)
            .child(ui, value);
        let meter = progress_meter(
            ui,
            "hud-health-meter",
            ratio,
            self.style.health,
            self.style.bar_background,
            self.style.outline,
        );

        ui.column()
            .id(ui, self.id)
            .style(ui, self.style.panel_style())
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, header)
            .child(ui, meter)
    }
}

struct AmmoHud<'a> {
    id: &'static str,
    style: HudStyle,
    weapon_name: &'a str,
    loaded: u32,
    capacity: u32,
    reload_remaining: f32,
    fire_cooldown_remaining: f32,
    gameplay_locked: bool,
    respawn_remaining: Option<f32>,
    size: UiSize,
    margin: EdgeInsets,
}

impl Widget for AmmoHud<'_> {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let label = hud_text(
            ui,
            "hud-ammo-label",
            self.weapon_name.to_uppercase(),
            self.style.label_style(self.style.muted),
        );
        let ammo = hud_text(
            ui,
            "hud-ammo-value",
            format!("{}/{}", self.loaded, self.capacity),
            self.style.value_style(self.style.ammo, 24.0),
        );
        let reload = hud_text(
            ui,
            "hud-ammo-reload",
            weapon_status_label(
                self.capacity,
                self.reload_remaining,
                self.fire_cooldown_remaining,
                self.gameplay_locked,
                self.respawn_remaining,
            ),
            self.style.label_style(self.style.text),
        );
        let loaded_ratio = if self.capacity > 0 {
            self.loaded as f32 / self.capacity as f32
        } else {
            0.0
        };
        let meter = progress_meter(
            ui,
            "hud-ammo-meter",
            loaded_ratio,
            self.style.ammo,
            self.style.bar_background,
            self.style.outline,
        );

        ui.column()
            .id(ui, self.id)
            .style(
                ui,
                self.style.panel_style().background(self.style.panel_strong),
            )
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, label)
            .child(ui, ammo)
            .child(ui, reload)
            .child(ui, meter)
    }
}

fn hud_text(
    ui: &mut UiArena,
    id: impl Into<String>,
    value: impl Into<String>,
    style: StylePatch,
) -> UiNodeHandle {
    ui.text(value)
        .id(ui, id.into())
        .style(ui, style)
        .pointer_events(ui, PointerEvents::None)
}

fn progress_meter(
    ui: &mut UiArena,
    id: impl Into<String>,
    value: f32,
    fill: UiColor,
    background: UiColor,
    outline: UiColor,
) -> UiNodeHandle {
    ui.progress_bar(value, fill)
        .id(ui, id.into())
        .background(ui, background)
        .border(ui, Border::solid(outline, 1.0))
        .radius(ui, CornerRadius::all(2.0))
        .width(ui, UiLength::Fill)
        .height(ui, UiLength::Px(12.0))
        .pointer_events(ui, PointerEvents::None)
}

fn weapon_status_label(
    capacity: u32,
    reload_remaining: f32,
    fire_cooldown_remaining: f32,
    gameplay_locked: bool,
    respawn_remaining: Option<f32>,
) -> String {
    if capacity == 0 {
        "DISABLED".to_string()
    } else if let Some(respawn_remaining) = respawn_remaining {
        format!("RESPAWN IN {respawn_remaining:.1}s")
    } else if gameplay_locked {
        "SYSTEM LOCKED".to_string()
    } else if reload_remaining > f32::EPSILON {
        format!("RELOADING {reload_remaining:.1}s")
    } else if fire_cooldown_remaining > f32::EPSILON {
        format!("COOLDOWN {fire_cooldown_remaining:.1}s")
    } else {
        "READY".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{tank_hud_state, weapon_status_label, ComponentTankHud, TankHudState};
    use crate::combat::{record_kill, ComponentCombatStats, ComponentKillFeed, KillEvent};
    use crate::spawn::ComponentRespawnState;
    use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
    use calyx_engine::component::ComponentID;
    use calyx_engine::math::Transform;
    use calyx_engine::scene::{Scene, SceneHud};
    use calyx_engine::ui::{PaintCommand, UiInput, UiPoint, UiRect, UiSize};

    fn tank_scene(respawn: ComponentRespawnState) -> (Scene, calyx_engine::scene::GameObject) {
        let mut scene = calyx_engine::test_support::test_scene();
        let tank = scene.create(None, None);
        scene.add_component(
            tank,
            ComponentHealth {
                current_health: 42.0,
                max_health: 90.0,
                ..Default::default()
            },
        );
        scene.add_component(
            tank,
            ComponentTankController {
                ammo: 2,
                max_ammo: 6,
                reload_remaining: 0.8,
                fire_cooldown_remaining: 0.2,
                ..Default::default()
            },
        );
        scene.add_component(tank, respawn);
        (scene, tank)
    }

    fn painted_text(scene: &Scene) -> Vec<String> {
        let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));
        SceneHud::default()
            .frame(scene, viewport, UiInput::default())
            .map(|frame| frame.paint_commands)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|command| match command {
                PaintCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tank_hud_state_reads_live_health_and_ammo() {
        let (scene, tank) = tank_scene(ComponentRespawnState {
            alive: false,
            respawn_remaining: 1.5,
            ..Default::default()
        });

        assert_eq!(
            tank_hud_state(&scene, tank),
            Some(TankHudState {
                health: 42.0,
                max_health: 90.0,
                ammo: 2,
                max_ammo: 6,
                reload_remaining: 0.8,
                fire_cooldown_remaining: 0.2,
                gameplay_locked: true,
                respawn_remaining: Some(1.5),
            })
        );
    }

    #[test]
    fn weapon_status_label_reports_gameplay_locks_and_cooldowns() {
        assert_eq!(weapon_status_label(6, 0.0, 0.0, false, None), "READY");
        assert_eq!(
            weapon_status_label(6, 1.25, 0.2, false, None),
            "RELOADING 1.2s"
        );
        assert_eq!(
            weapon_status_label(6, 0.0, 0.35, false, None),
            "COOLDOWN 0.3s"
        );
        assert_eq!(
            weapon_status_label(6, 1.25, 0.35, true, None),
            "SYSTEM LOCKED"
        );
        assert_eq!(
            weapon_status_label(6, 0.0, 0.0, true, Some(2.04)),
            "RESPAWN IN 2.0s"
        );
        assert_eq!(weapon_status_label(0, 0.0, 0.0, false, None), "DISABLED");
    }

    #[test]
    fn tank_hud_component_paints_health_ammo_and_reload() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());

        let text = painted_text(&scene);

        for expected in [
            "HULL INTEGRITY",
            "042/090",
            "CANNON",
            "2/6",
            "RELOADING 0.8s",
        ] {
            assert!(text.iter().any(|t| t == expected), "{expected} in {text:?}");
        }
    }

    #[test]
    fn hud_panels_fit_their_text_without_shrinking_it() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());
        let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));
        let frame = SceneHud::default()
            .frame(&scene, viewport, UiInput::default())
            .expect("the tank should draw a HUD");

        for command in &frame.paint_commands {
            if let PaintCommand::Text {
                rect,
                text,
                font_size,
                ..
            } = command
            {
                // egui paints a line of text this tall whatever its rect, so a
                // shorter rect overlaps the next row.
                let height = rect.max.y - rect.min.y;
                assert!(
                    height >= font_size * 1.25 - 1e-3,
                    "{text} is {height} tall at font size {font_size}"
                );
            }
        }
    }

    #[test]
    fn tank_hud_component_counts_down_to_respawn() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState {
            alive: false,
            respawn_remaining: 2.5,
            ..Default::default()
        });
        scene.add_component(tank, ComponentTankHud::default());

        assert!(painted_text(&scene).contains(&"RESPAWN IN 2.5s".to_string()));
    }

    #[test]
    fn tank_hud_paints_the_kill_feed_newest_first() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());
        scene.add_component(tank, ComponentKillFeed::default());
        for victim in ["Target A", "Target B"] {
            let target = scene.create(
                Some(ComponentID {
                    name: victim.to_string(),
                    ..Default::default()
                }),
                None,
            );
            let event = KillEvent::new(&scene, Some(tank), target);
            record_kill(&mut scene, &event);
        }

        let text = painted_text(&scene);
        let victims: Vec<&String> = text
            .iter()
            .filter(|text| text.starts_with("TARGET"))
            .collect();
        assert_eq!(victims, vec!["TARGET B", "TARGET A"]);
        assert_eq!(
            text.iter().filter(|text| *text == "destroyed").count(),
            2,
            "{text:?}"
        );
    }

    #[test]
    fn tank_hud_shows_the_scoreboard_only_while_open() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());
        scene.add_component(
            tank,
            ComponentCombatStats {
                kills: 3,
                deaths: 1,
            },
        );

        assert!(!painted_text(&scene).contains(&"SCOREBOARD".to_string()));

        let _ = scene.write_component::<ComponentTankHud, _>(tank, |hud| {
            hud.scoreboard_open = true;
        });
        let text = painted_text(&scene);
        for expected in ["SCOREBOARD", "KILLS", "DEATHS", "3", "1"] {
            assert!(text.iter().any(|t| t == expected), "{expected} in {text:?}");
        }
    }

    #[test]
    fn tank_hud_draws_the_minimap() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());
        let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));
        let frame = SceneHud::default()
            .frame(&scene, viewport, UiInput::default())
            .expect("the tank should draw a HUD");

        assert!(frame.paint_commands.iter().any(
            |command| matches!(command, PaintCommand::Text { text, .. } if text == "TACTICAL MAP")
        ));
        assert!(
            frame
                .paint_commands
                .iter()
                .any(|command| matches!(command, PaintCommand::Line { .. })),
            "the player's heading is drawn"
        );
    }

    #[test]
    fn tank_hud_clips_the_minimap_to_its_panel() {
        let (mut scene, tank) = tank_scene(ComponentRespawnState::default());
        scene.add_component(tank, ComponentTankHud::default());
        // No arena, so the map shows 25 m around the tank: this target is
        // far off the map.
        let target = scene.create(None, None);
        scene.set_world_transform(target, Transform::from_xyz(1000.0, 0.0, 0.0).matrix());
        scene.add_component(target, ComponentProjectileTarget::default());
        let viewport = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(1280.0, 720.0));
        let commands = SceneHud::default()
            .frame(&scene, viewport, UiInput::default())
            .expect("the tank should draw a HUD")
            .paint_commands;

        let label = commands
            .iter()
            .position(|command| {
                matches!(command, PaintCommand::Text { text, .. } if text == "TACTICAL MAP")
            })
            .expect("the minimap is drawn");
        let (start, clip) = commands
            .iter()
            .enumerate()
            .skip(label)
            .find_map(|(index, command)| match command {
                PaintCommand::PushClip(rect) => Some((index, *rect)),
                _ => None,
            })
            .expect("the map clips what it paints");
        let end = start
            + commands[start..]
                .iter()
                .position(|command| matches!(command, PaintCommand::PopClip))
                .expect("the map's clip ends");
        let outside = |rect: &UiRect| {
            rect.max.x < clip.min.x
                || rect.min.x > clip.max.x
                || rect.max.y < clip.min.y
                || rect.min.y > clip.max.y
        };
        let off_map: Vec<usize> = commands
            .iter()
            .enumerate()
            .skip(label)
            .filter(|(_, command)| {
                matches!(command, PaintCommand::FillRect { rect, .. } if outside(rect))
            })
            .map(|(index, _)| index)
            .collect();
        assert!(!off_map.is_empty(), "the far target is painted off the map");
        assert!(
            off_map.iter().all(|index| (start..end).contains(index)),
            "off-map blips at {off_map:?} paint outside the clip {start}..{end}"
        );
    }

    #[test]
    fn tank_without_hud_component_paints_nothing() {
        let (scene, _) = tank_scene(ComponentRespawnState::default());

        assert!(painted_text(&scene).is_empty());
    }
}
