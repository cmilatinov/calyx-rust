mod kill_feed;
mod minimap;
mod paint;
mod scoreboard;

use crate::combat::{combat_team, ComponentCombatStats};
use crate::game_state::ComponentGameState;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentDepthOfField, ComponentEventContext, ComponentHud, ComponentID,
    ComponentLocalPostProcessVolume, ComponentRef, ComponentUpdate, LocalVolumeShape,
    ReflectComponent, ReflectComponentHud, ReflectComponentUpdate,
};
use calyx_engine::input::Input;
use calyx_engine::math::Transform;
use calyx_engine::reflect::{Reflect, ReflectDefault};
use calyx_engine::resource::ResourceMap;
use calyx_engine::scene::{GameObject, GameObjectRef, Scene};
use calyx_engine::ui::{
    PaintCommand, PointerEvents, UiArena, UiColor, UiLength, UiNodeHandle, UiRect,
};
use calyx_engine::utils::{ReflectTypeUuidDynamic, TypeUuid};
use nalgebra_glm::{vec2, vec3, vec4, Mat4, Vec3};
use paint::{Painter, BLACK, CYAN, LIGHT, MUTED, RED, STEEL};
use serde::{Deserialize, Serialize};

/// Concept-driven presentation over real scene combat state. No input is
/// captured by the HUD; the scoreboard follows the existing held action.
#[derive(Clone, Copy, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b7f3a8d2-5c41-4e8b-9a6f-0d2e7c1b4f93"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentHud)]
#[reflect_attr(name = "Tank HUD")]
#[serde(default)]
pub struct ComponentTankHud {
    pub screen_margin: f32,
    /// Standalone scenes count down this duration; zero selects elapsed time.
    /// A scene Game State component supplies the authoritative clock instead.
    pub timer_duration: f32,
    pub vision_range: f32,
    pub low_damage_ratio: f32,
    pub high_damage_ratio: f32,
    #[serde(skip)]
    #[reflect_skip]
    pub scoreboard_open: bool,
    #[serde(skip)]
    #[reflect_skip]
    elapsed: f32,
    #[serde(skip)]
    #[reflect_skip]
    scoreboard_page: usize,
    #[serde(skip)]
    #[reflect_skip]
    scoreboard_blur: GameObjectRef,
}
impl Default for ComponentTankHud {
    fn default() -> Self {
        Self {
            screen_margin: 34.0,
            timer_duration: 300.0,
            vision_range: 15.0,
            low_damage_ratio: 0.15,
            high_damage_ratio: 0.5,
            scoreboard_open: false,
            elapsed: 0.0,
            scoreboard_page: 0,
            scoreboard_blur: Default::default(),
        }
    }
}
impl Component for ComponentTankHud {}
impl ComponentTankHud {
    fn advance(&mut self, dt: f32) {
        if dt.is_finite() {
            self.elapsed += dt.max(0.0);
        }
    }
    fn time(&self, scene: &Scene) -> f32 {
        scene
            .objects()
            .find_map(|object| {
                scene.read_component::<ComponentGameState, _, _>(object, |state| {
                    if state.match_duration > 0.0 {
                        state.match_time_remaining.max(0.0)
                    } else {
                        state.phase_elapsed.max(0.0).floor()
                    }
                })
            })
            .unwrap_or_else(|| {
                if self.timer_duration > 0.0 {
                    (self.timer_duration - self.elapsed).max(0.0)
                } else {
                    self.elapsed.floor()
                }
            })
    }
}
impl ComponentUpdate for ComponentTankHud {
    fn update(
        this: ComponentRef<Self>,
        ComponentEventContext { scene, .. }: ComponentEventContext,
        resources: &mut ResourceMap,
        input: &Input,
    ) {
        let open = input.action("scoreboard").pressed();
        let pages = scoreboard::page_count(&scoreboard::scoreboard_rows(scene, this.game_object()));
        let _ = this.write(scene, |hud| {
            if !open {
                hud.scoreboard_page = 0;
            } else if input.scroll_delta().y < 0.0 {
                hud.scoreboard_page = hud.scoreboard_page.saturating_add(1);
            } else if input.scroll_delta().y > 0.0 {
                hud.scoreboard_page = hud.scoreboard_page.saturating_sub(1);
            }
            hud.scoreboard_page = hud.scoreboard_page.min(pages - 1);
            hud.scoreboard_open = open;
            hud.advance(resources.time().delta_time());
        });
        let previous = this
            .read(scene, |hud| hud.scoreboard_blur)
            .unwrap_or_default();
        let blur = update_scoreboard_blur(scene, this.game_object(), open, previous);
        let _ = this.write(scene, |hud| hud.scoreboard_blur = blur);
        // The aim object remains the world-space source. Its legacy mesh would
        // otherwise overlap the projected ground ring drawn by this HUD.
        if let Some(marker) = scene
            .read_component::<ComponentTankController, _, _>(this.game_object(), |controller| {
                controller.crosshair
            })
            .and_then(|marker| marker.game_object(scene))
        {
            let _ = scene.write_component::<ComponentID, _>(marker, |id| id.visible = false);
        }
    }
}
/// A transient local volume changes only depth of field at the active camera;
/// removing it restores the scene's existing post-processing without editing assets.
fn update_scoreboard_blur(
    scene: &mut Scene,
    owner: GameObject,
    open: bool,
    previous: GameObjectRef,
) -> GameObjectRef {
    let camera_position = scene
        .main_camera()
        .map(|(camera, _)| scene.world_transform(camera).position);
    if !open || camera_position.is_none() {
        if let Some(object) = previous.game_object(scene) {
            scene.delete(object);
        }
        return Default::default();
    }
    let object = previous.game_object(scene).unwrap_or_else(|| {
        let object = scene.create(
            Some(ComponentID {
                name: "Scoreboard Backdrop".to_string(),
                ..Default::default()
            }),
            Some(owner),
        );
        scene.add_component(
            object,
            ComponentLocalPostProcessVolume {
                shape: LocalVolumeShape::Sphere { radius: 0.25 },
                blend_distance: 0.0,
                weight: 1.0,
                priority: i32::MAX,
                ..Default::default()
            },
        );
        scene.add_component(
            object,
            ComponentDepthOfField {
                focus_distance: 0.0,
                focus_range: 0.01,
                max_blur: 16.0,
            },
        );
        object
    });
    scene.set_world_transform(
        object,
        Transform::from_xyz(
            camera_position.unwrap().x,
            camera_position.unwrap().y,
            camera_position.unwrap().z,
        )
        .matrix(),
    );
    GameObjectRef::new(scene.uuid(object))
}
impl ComponentHud for ComponentTankHud {
    fn build_hud(
        hud: &Self,
        scene: &Scene,
        player: GameObject,
        ui: &mut UiArena,
        viewport: UiRect,
    ) -> Option<UiNodeHandle> {
        let state = tank_hud_state(scene, player)?;
        let mut p = Painter::new(viewport);
        let width = viewport.width() / p.scale;
        let height = viewport.height() / p.scale;
        let margin = hud.screen_margin.max(12.0).min(width * 0.1);
        let map = minimap::minimap_state(scene, player);
        if !hud.scoreboard_open {
            paint_world(&mut p, scene, player, hud, &map, width, height);
            kill_feed::paint_feed(&mut p, scene, player, margin, margin);
            minimap::paint_map(
                &mut p,
                &map,
                margin + 221.0,
                margin + 381.0,
                215.0,
                hud.vision_range,
            );
            p.panel(width - margin - 260.0, margin, 260.0, 90.0, 18.0);
            p.bold_centered(
                width - margin - 130.0,
                margin + 12.0,
                &format_time(hud.time(scene)),
                52.0,
                LIGHT,
            );
            // Compact retained instrumentation complements the reference's world HP.
            p.panel(margin, height - margin - 73.0, 222.0, 73.0, 12.0);
            p.text(margin + 15.0, height - margin - 67.0, "HULL", 14.0, MUTED);
            p.text(
                margin + 91.0,
                height - margin - 69.0,
                format!(
                    "{:03.0}/{:03.0}",
                    state.health.max(0.0),
                    state.max_health.max(0.0)
                ),
                20.0,
                LIGHT,
            );
            meter(
                &mut p,
                margin + 15.0,
                height - margin - 28.0,
                192.0,
                12.0,
                health_ratio(state.health, state.max_health),
                CYAN,
            );
            let ax = width - margin - 260.0;
            let ay = height - margin - 93.0;
            p.panel(ax, ay, 260.0, 93.0, 12.0);
            p.bold_text(ax + 16.0, ay + 9.0, "CANNON", 15.0, MUTED);
            p.text(
                ax + 171.0,
                ay + 3.0,
                format!("{}/{}", state.ammo, state.max_ammo),
                29.0,
                LIGHT,
            );
            p.text(
                ax + 16.0,
                ay + 42.0,
                weapon_status_label(
                    state.max_ammo,
                    state.reload_remaining,
                    state.fire_cooldown_remaining,
                    state.gameplay_locked,
                    state.respawn_remaining,
                ),
                16.0,
                if state.gameplay_locked { RED } else { LIGHT },
            );
            let reload_duration = scene
                .read_component::<ComponentTankController, _, _>(player, |controller| {
                    controller.reload_duration
                })
                .unwrap_or(1.0);
            let ratio = if state.reload_remaining > 0.0 {
                1.0 - state.reload_remaining / reload_duration.max(f32::EPSILON)
            } else {
                state.ammo as f32 / state.max_ammo.max(1) as f32
            };
            meter(&mut p, ax + 16.0, ay + 72.0, 228.0, 9.0, ratio, CYAN);
        }
        if hud.scoreboard_open {
            scoreboard::paint_scoreboard(
                &mut p,
                &scoreboard::scoreboard_rows(scene, player),
                width,
                height,
                hud.scoreboard_page,
            );
        }
        p.commands.push(PaintCommand::PopClip);
        Some(
            ui.custom_paint(p.commands)
                .id(ui, "sandbox-ui")
                .width(ui, UiLength::Px(viewport.width()))
                .height(ui, UiLength::Px(viewport.height()))
                .pointer_events(ui, PointerEvents::None),
        )
    }
}
fn format_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0).ceil() as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
fn health_ratio(health: f32, max: f32) -> f32 {
    if max > 0.0 {
        (health / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn meter(p: &mut Painter, x: f32, y: f32, w: f32, h: f32, ratio: f32, color: UiColor) {
    p.fill(x - 2.0, y - 2.0, w + 4.0, h + 4.0, BLACK, h * 0.5);
    p.fill(x, y, w, h, STEEL.with_alpha(0.5), h * 0.5);
    let fill = w * ratio.clamp(0.0, 1.0);
    if fill > 0.0 {
        p.fill(x, y, fill, h, color, h * 0.5);
    }
}

struct Projection {
    matrix: Mat4,
    width: f32,
    height: f32,
}
impl Projection {
    fn scene(scene: &Scene, width: f32, height: f32) -> Option<Self> {
        let (object, camera) = scene.main_camera()?;
        Some(Self {
            matrix: camera.camera(width / height).projection
                * scene.world_transform(object).inverse_matrix(),
            width,
            height,
        })
    }
    fn point(&self, position: Vec3) -> Option<(f32, f32)> {
        let clip = self.matrix * vec4(position.x, position.y, position.z, 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let n = clip.xyz() / clip.w;
        if !n.iter().all(|v| v.is_finite()) || n.z < 0.0 || n.z > 1.0 {
            return None;
        }
        Some((
            (n.x + 1.0) * 0.5 * self.width,
            (1.0 - n.y) * 0.5 * self.height,
        ))
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum DamageTier {
    Low,
    Normal,
    High,
}
fn damage_tier(amount: f32, max: f32, low: f32, high: f32) -> DamageTier {
    let ratio = amount / max.max(f32::EPSILON);
    if ratio >= high.max(low).max(0.0) {
        DamageTier::High
    } else if ratio < low.max(0.0) {
        DamageTier::Low
    } else {
        DamageTier::Normal
    }
}
fn paint_world(
    p: &mut Painter,
    scene: &Scene,
    player: GameObject,
    hud: &ComponentTankHud,
    map: &minimap::MinimapState,
    width: f32,
    height: f32,
) {
    let Some(projection) = Projection::scene(scene, width, height) else {
        return;
    };
    let player_team = combat_team(scene, player);
    for object in scene.objects() {
        if !scene.is_visible_in_hierarchy(object) {
            continue;
        }
        let Some(health) = scene
            .read_component::<ComponentHealth, _, _>(object, |health| *health)
            .filter(|health| !health.dead && health.max_health > 0.0)
        else {
            continue;
        };
        let position = scene.world_transform(object).position;
        let friendly = object == player
            || player_team
                .zip(combat_team(scene, object))
                .is_some_and(|(a, b)| a == b);
        if !friendly && !map.visible(vec2(position.x, position.z), hud.vision_range) {
            continue;
        }
        let Some((x, y)) = projection.point(position + vec3(0.0, 2.4, 0.0)) else {
            continue;
        };
        p.fill(x - 120.0, y - 5.0, 240.0, 30.0, BLACK, 15.0);
        p.fill(x - 117.0, y - 2.0, 234.0, 24.0, STEEL, 12.0);
        meter(
            p,
            x - 113.0,
            y + 2.0,
            226.0,
            16.0,
            health_ratio(health.current_health, health.max_health),
            if friendly { CYAN } else { RED },
        );
        if !friendly {
            p.polygon(
                &[(x - 7.0, y + 30.0), (x + 7.0, y + 30.0), (x, y + 40.0)],
                RED,
            );
        }
    }
    let aim = scene
        .read_component::<ComponentTankController, _, _>(player, |controller| controller.crosshair)
        .and_then(|marker| marker.game_object(scene))
        .map(|marker| scene.world_transform(marker).position);
    if let Some(aim) = aim {
        let origin = vec3(aim.x, aim.y + 0.035, aim.z);
        for i in 0..80 {
            let a = i as f32 * std::f32::consts::TAU / 80.0;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 80.0;
            if let (Some(a), Some(b)) = (
                projection.point(origin + vec3(a.cos() * 0.85, 0.0, a.sin() * 0.85)),
                projection.point(origin + vec3(b.cos() * 0.85, 0.0, b.sin() * 0.85)),
            ) {
                p.line(a, b, BLACK.with_alpha(0.65), 5.0);
                p.line(a, b, LIGHT, 2.2);
            }
        }
        for direction in [
            vec3(1.0, 0.0, 0.0),
            vec3(-1.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            vec3(0.0, 0.0, -1.0),
        ] {
            if let (Some(a), Some(b)) = (
                projection.point(origin + direction * 1.05),
                projection.point(origin + direction * 1.45),
            ) {
                p.line(a, b, BLACK.with_alpha(0.65), 6.0);
                p.line(a, b, LIGHT, 3.0);
            }
        }
        if let Some((x, y)) = projection.point(origin) {
            p.circle(x, y, 2.0, LIGHT);
        }
    }
    let _ = scene.read_component::<ComponentCombatStats, _, _>(player, |stats| {
        for (index, hit) in stats.feedback.iter().enumerate() {
            if hit.age >= 0.9 {
                continue;
            }
            let Some((mut x, mut y)) = projection.point(hit.position + vec3(0.0, 2.0, 0.0)) else {
                continue;
            };
            x += ((index % 3) as f32 - 1.0) * 20.0;
            y -= 28.0 + hit.age * 70.0;
            let tier = damage_tier(
                hit.amount,
                hit.max_health,
                hud.low_damage_ratio,
                hud.high_damage_ratio,
            );
            let (size, color) = match tier {
                DamageTier::High => (43.0, RED),
                DamageTier::Normal => (32.0, LIGHT),
                DamageTier::Low => (25.0, MUTED),
            };
            let number = if hit.amount < 1.0 {
                format!("-{:.1}", hit.amount)
            } else {
                format!("-{:.0}", hit.amount)
            };
            let text = if tier == DamageTier::High {
                format!("{number}!")
            } else {
                number
            };
            let opacity = ((0.9 - hit.age) / 0.3).clamp(0.0, 1.0);
            for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                p.centered(x + dx, y + dy, &text, size, BLACK.with_alpha(opacity));
            }
            p.centered(x, y, &text, size, color.with_alpha(opacity));
        }
        if let Some(hit) = stats
            .feedback
            .iter()
            .filter(|hit| hit.age < 0.16)
            .min_by(|a, b| a.age.total_cmp(&b.age))
        {
            let (x, y) = aim
                .and_then(|aim| projection.point(aim))
                .unwrap_or((width * 0.5, height * 0.5));
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                p.line(
                    (x + dx * 9.0, y + dy * 9.0),
                    (x + dx * 19.0, y + dy * 19.0),
                    LIGHT.with_alpha(1.0 - hit.age / 0.16),
                    3.0,
                );
            }
        }
    });
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
    use super::*;
    use calyx_engine::scene::SceneHud;
    use calyx_engine::ui::{UiInput, UiPoint, UiSize};
    fn setup() -> (Scene, GameObject) {
        let mut scene = calyx_engine::test_support::test_scene();
        let tank = scene.create(None, None);
        scene.add_component(
            tank,
            ComponentTankController {
                ammo: 2,
                max_ammo: 6,
                reload_remaining: 0.8,
                ..Default::default()
            },
        );
        scene.add_component(
            tank,
            ComponentHealth {
                current_health: 42.0,
                max_health: 90.0,
                ..Default::default()
            },
        );
        scene.add_component(tank, ComponentTankHud::default());
        (scene, tank)
    }
    fn commands(scene: &Scene, width: f32, height: f32) -> Vec<PaintCommand> {
        SceneHud::default()
            .frame(
                scene,
                UiRect::from_min_size(UiPoint::new(30.0, 50.0), UiSize::new(width, height)),
                UiInput::default(),
            )
            .unwrap()
            .paint_commands
    }
    #[test]
    fn live_health_ammo_reload_and_timer_are_painted_at_both_viewport_sizes() {
        let (scene, _) = setup();
        for (width, height) in [(1672.0, 941.0), (960.0, 540.0)] {
            let commands = commands(&scene, width, height);
            let text = commands
                .iter()
                .filter_map(|c| {
                    if let PaintCommand::Text { text, .. } = c {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for expected in [
                "HULL",
                "042/090",
                "CANNON",
                "2/6",
                "RELOADING 0.8s",
                "05:00",
                "N",
            ] {
                assert!(text.contains(&expected), "{expected} in {text:?}");
            }
            for command in commands {
                if let PaintCommand::Text {
                    rect, font_size, ..
                } = command
                {
                    assert!(rect.height() >= font_size * 1.25);
                }
            }
        }
    }
    #[test]
    fn timer_uses_simulation_delta_resets_with_scene_and_reads_match_state() {
        let (scene, _) = setup();
        let mut hud = ComponentTankHud::default();
        hud.advance(1.2);
        assert_eq!(format_time(hud.time(&scene)), "04:59");
        hud.advance(0.0);
        assert!((hud.time(&scene) - 298.8).abs() < 0.001);
        hud.advance(-1.0);
        hud.advance(f32::NAN);
        assert!((hud.time(&scene) - 298.8).abs() < 0.001);
        let restored: ComponentTankHud =
            serde_json::from_value(serde_json::to_value(hud).unwrap()).unwrap();
        assert_eq!(restored.elapsed, 0.0);
        hud.timer_duration = 0.0;
        assert_eq!(format_time(hud.time(&scene)), "00:01");
        let mut scene = scene;
        let state = scene.create(None, None);
        scene.add_component(
            state,
            ComponentGameState {
                match_time_remaining: 123.0,
                ..Default::default()
            },
        );
        assert_eq!(format_time(hud.time(&scene)), "02:03");
        scene.write_component::<ComponentGameState, _>(state, |state| {
            state.match_duration = 0.0;
            state.phase_elapsed = 42.0;
        });
        assert_eq!(format_time(hud.time(&scene)), "00:42");
    }
    #[test]
    fn damage_tiers_are_removed_health_ratios_and_weapon_locks_remain_truthful() {
        assert_eq!(damage_tier(10.0, 100.0, 0.15, 0.5), DamageTier::Low);
        assert_eq!(damage_tier(35.0, 100.0, 0.15, 0.5), DamageTier::Normal);
        assert_eq!(damage_tier(60.0, 100.0, 0.15, 0.5), DamageTier::High);
        assert_eq!(weapon_status_label(6, 0.0, 0.0, false, None), "READY");
        assert_eq!(
            weapon_status_label(6, 0.0, 0.3, false, None),
            "COOLDOWN 0.3s"
        );
        assert_eq!(
            weapon_status_label(6, 0.0, 0.0, true, Some(2.5)),
            "RESPAWN IN 2.5s"
        );
    }
    #[test]
    fn scoreboard_is_held_and_bounded_with_many_real_rows() {
        let (mut scene, tank) = setup();
        for i in 0..80 {
            let actor = scene.create(None, None);
            scene.add_component(
                actor,
                ComponentCombatStats {
                    team: i % 2,
                    ..Default::default()
                },
            );
        }
        assert!(!commands(&scene, 960.0, 540.0)
            .iter()
            .any(|c| matches!(c,PaintCommand::Text{text,..}if text=="SCOREBOARD")));
        scene.write_component::<ComponentTankHud, _>(tank, |hud| hud.scoreboard_open = true);
        let commands = commands(&scene, 960.0, 540.0);
        assert!(commands
            .iter()
            .any(|c| matches!(c,PaintCommand::Text{text,..}if text=="SCOREBOARD")));
        assert!(commands
            .iter()
            .any(|c| matches!(c,PaintCommand::Text{text,..}if text.starts_with("PAGE 1/"))));
        for command in commands {
            if let PaintCommand::Text { rect, .. } = command {
                assert!(rect.max.y <= 590.0, "text overflow {rect:?}");
            }
        }
    }
    #[test]
    fn scoreboard_blur_is_local_reused_and_removed_without_touching_world_effects() {
        let (mut scene, owner) = setup();
        let camera = scene.create(None, None);
        scene.add_component(camera, calyx_engine::component::ComponentCamera::default());
        let before = scene.objects().count();
        let blur = update_scoreboard_blur(&mut scene, owner, true, Default::default());
        let object = blur.game_object(&scene).unwrap();
        assert_eq!(scene.parent(object), Some(owner));
        assert_eq!(scene.objects().count(), before + 1);
        assert_eq!(
            scene.read_component::<ComponentLocalPostProcessVolume, _, _>(object, |volume| volume
                .weight),
            Some(1.0)
        );
        let reused = update_scoreboard_blur(&mut scene, owner, true, blur);
        assert_eq!(reused.game_object(&scene), Some(object));
        update_scoreboard_blur(&mut scene, owner, false, blur);
        scene.flush_deletes();
        assert_eq!(scene.objects().count(), before);
        let blur = update_scoreboard_blur(&mut scene, owner, true, Default::default());
        scene.delete(owner);
        scene.flush_deletes();
        assert!(blur.game_object(&scene).is_none());
    }
}
