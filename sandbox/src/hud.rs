mod fog;
mod kill_feed;
mod minimap;
mod paint;
mod scoreboard;

use crate::combat::{combat_team, ComponentCombatStats};
use crate::game_state::ComponentGameState;
use crate::tank::{ComponentHealth, ComponentTankController};
use calyx_engine::component::{
    Component, ComponentDepthOfField, ComponentEventContext, ComponentHud, ComponentID,
    ComponentLocalPostProcessVolume, ComponentOutline, ComponentRef, ComponentUpdate,
    LocalVolumeShape, ReflectComponent, ReflectComponentHud, ReflectComponentUpdate,
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
use nalgebra_glm::{vec2, vec4, Mat4, Vec3};
use paint::{Painter, BLACK, CYAN, LIGHT, MUTED, RED};
use serde::{Deserialize, Serialize};

/// Concept-driven presentation over real scene combat state. No input is
/// captured by the HUD; the scoreboard follows the existing held action.
#[derive(Clone, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "b7f3a8d2-5c41-4e8b-9a6f-0d2e7c1b4f93"]
#[reflect(Default, TypeUuidDynamic, Component, ComponentUpdate, ComponentHud)]
#[reflect_attr(name = "Tank HUD")]
#[serde(default)]
pub struct ComponentTankHud {
    pub screen_margin: f32,
    /// Standalone scenes count down this duration; zero selects elapsed time.
    /// A scene Game State component supplies the authoritative clock instead.
    pub timer_duration: f32,
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
    #[serde(skip)]
    #[reflect_skip]
    map_boundary: std::sync::Arc<std::sync::Mutex<minimap::BoundaryCache>>,
    /// What fog of war hid from this HUD's player.
    #[serde(skip)]
    #[reflect_skip]
    pub(crate) fog: crate::vision::FogState,
    /// The fog painted last frame, so the next one eases from it.
    #[serde(skip)]
    #[reflect_skip]
    fog_fade: std::sync::Arc<std::sync::Mutex<fog::FogFade>>,
}

impl Default for ComponentTankHud {
    fn default() -> Self {
        Self {
            screen_margin: 34.0,
            timer_duration: 300.0,
            low_damage_ratio: 0.15,
            high_damage_ratio: 0.5,
            scoreboard_open: false,
            elapsed: 0.0,
            scoreboard_page: 0,
            scoreboard_blur: Default::default(),
            map_boundary: Default::default(),
            fog: Default::default(),
            fog_fade: Default::default(),
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
        let dt = resources.time().delta_time();
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
            hud.advance(dt);
        });
        let previous = this
            .read(scene, |hud| hud.scoreboard_blur)
            .unwrap_or_default();
        let blur = update_scoreboard_blur(scene, this.game_object(), open, previous);
        let _ = this.write(scene, |hud| hud.scoreboard_blur = blur);
        if let Some(mut fog) = this.write(scene, |hud| std::mem::take(&mut hud.fog)) {
            fog.advance(dt);
            crate::vision::apply_fog(scene, this.game_object(), &mut fog);
            let _ = this.write(scene, |hud| hud.fog = fog);
        }
    }
}

/// A transient local volume defocuses the backdrop and suppresses late ink;
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
        // Ink is composited after DOF. Fade its styles out only in this local
        // overlay volume so sharp silhouettes cannot redraw over the blur.
        scene.add_component(object, ComponentOutline::default());
        scene.add_component(
            object,
            ComponentDepthOfField {
                focus_distance: 0.0,
                focus_range: 0.01,
                max_blur: 30.0,
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
        scene.read_component::<ComponentTankController, _, _>(player, |_| ())?;
        let mut p = Painter::new(viewport);
        let width = viewport.width() / p.scale;
        let height = viewport.height() / p.scale;
        let margin = hud.screen_margin.max(12.0).min(width * 0.1);
        let mut map = minimap::minimap_state(scene, player);
        for blip in &mut map.blips {
            blip.presence = hud.fog.presence(blip.id);
        }
        if !hud.scoreboard_open {
            fog::paint_fog(
                &mut p,
                scene,
                &map.vision,
                scene.world_transform(player).position.y,
                width,
                height,
                &mut hud
                    .fog_fade
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                hud.elapsed,
            );
            paint_world(&mut p, scene, player, hud, &map, width, height);
            let feed_width = (width * 0.24).clamp(230.0, 390.0);
            let feed_height = (height * 0.043).clamp(28.0, 42.0);
            kill_feed::paint_feed(
                &mut p,
                scene,
                player,
                width - margin - feed_width,
                margin,
                feed_width,
                feed_height,
            );
            let radius = (width * 0.068).clamp(66.0, 128.0).min(height * 0.18);
            minimap::paint_map(
                &mut p,
                &map,
                &hud.map_boundary,
                margin + radius + 4.0,
                margin + radius + 14.0,
                radius,
            );
            let tw = (width * 0.16).clamp(160.0, 270.0) * 0.82;
            let th = (height * 0.09).clamp(60.0, 88.0) * 0.82;
            p.plate((width - tw) * 0.5, margin, tw, th);
            p.middle(
                width * 0.5,
                margin + th * 0.51,
                format_time(hud.time(scene)),
                th * 0.61,
                LIGHT,
                None,
            );
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

#[derive(Clone, Copy, Debug)]
struct ScreenBounds {
    left: f32,
    right: f32,
    top: f32,
}

impl Projection {
    fn visual_bounds(&self, scene: &Scene, object: GameObject) -> Option<ScreenBounds> {
        let mut points = scene
            .visual_bound_corners(object)
            .into_iter()
            .filter_map(|point| self.point(point));
        let (x, y) = points.next()?;
        let mut bounds = ScreenBounds {
            left: x,
            right: x,
            top: y,
        };
        for (x, y) in points {
            bounds.left = bounds.left.min(x);
            bounds.right = bounds.right.max(x);
            bounds.top = bounds.top.min(y);
        }
        Some(bounds)
    }
}

/// Fill width of every world health bar, in HUD units. Fixed so a bar never
/// grows or shrinks with the entity's size, heading or distance; it matches
/// the player tank's bar at spawn heading under the default chase camera.
const HP_BAR_WIDTH: f32 = 64.0;
/// Fill thickness of every world health bar, in HUD units.
const HP_BAR_THICKNESS: f32 = 7.0;

/// Centre and width of an entity's health bar. Only the position follows the
/// projected bounds: centred over them, just above their top edge.
fn hp_anchor(bounds: ScreenBounds) -> (f32, f32, f32) {
    (
        (bounds.left + bounds.right) * 0.5,
        bounds.top - 8.0,
        HP_BAR_WIDTH,
    )
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
            .read_component::<ComponentHealth, _, _>(object, |h| *h)
            .filter(|h| !h.dead && h.max_health > 0.0)
        else {
            continue;
        };
        let position = scene.world_transform(object).position;
        let friendly = object == player
            || player_team
                .zip(combat_team(scene, object))
                .is_some_and(|(a, b)| a == b);
        // An enemy's bar fades with it as fog closes over or clears from it.
        let opacity = if friendly {
            1.0
        } else {
            let visible = map.vision.visible(vec2(position.x, position.z));
            hud.fog
                .presence(scene.uuid(object))
                .unwrap_or(if visible { 1.0 } else { 0.0 })
        };
        if opacity <= 0.0 {
            continue;
        }
        let faded = |color: UiColor| color.with_alpha(color.a as f32 / 255.0 * opacity);
        let Some(bounds) = projection.visual_bounds(scene, object) else {
            continue;
        };
        let (x, y, w) = hp_anchor(bounds);
        let h = HP_BAR_THICKNESS;
        let outer_radius = (h * 0.5 + 3.0) * 0.78;
        p.fill(
            x - w * 0.5 - 3.0,
            y - h * 0.5 - 3.0,
            w + 6.0,
            h + 6.0,
            faded(BLACK),
            outer_radius,
        );
        let left = x - w * 0.5 - 1.0;
        let top = y - h * 0.5 - 1.0;
        let inner_w = w + 2.0;
        let inner_h = h + 2.0;
        p.fill(
            left,
            top,
            inner_w,
            inner_h,
            faded(paint::PANEL),
            outer_radius - 2.0,
        );
        let trail = health
            .damage_trail
            .edge(health.current_health, health.max_health, health.dead);
        let trailing_width = inner_w * health_ratio(trail, health.max_health);
        let fill = inner_w * health_ratio(health.current_health, health.max_health);
        if trailing_width > fill {
            p.fill(
                left,
                top,
                trailing_width,
                inner_h,
                faded(LIGHT),
                outer_radius - 2.0,
            );
        }
        if fill > 0.0 {
            p.fill(
                left,
                top,
                fill,
                inner_h,
                faded(if friendly { CYAN } else { RED }),
                outer_radius - 2.0,
            );
        }
        if !friendly {
            p.polygon(
                &[
                    (x - 8.0, y + h * 0.5 + 7.0),
                    (x + 8.0, y + h * 0.5 + 7.0),
                    (x, y + h * 0.5 + 19.0),
                ],
                faded(BLACK),
            );
            p.polygon(
                &[
                    (x - 5.5, y + h * 0.5 + 8.5),
                    (x + 5.5, y + h * 0.5 + 8.5),
                    (x, y + h * 0.5 + 16.0),
                ],
                faded(RED),
            );
        }
    }
    let aim = scene
        .read_component::<ComponentTankController, _, _>(player, |c| c.crosshair)
        .and_then(|r| r.game_object(scene))
        .and_then(|o| projection.point(scene.world_transform(o).position));
    let _ = scene.read_component::<ComponentCombatStats, _, _>(player, |stats| {
        for (index, hit) in stats.feedback.iter().enumerate() {
            if hit.age >= crate::combat::DAMAGE_FEEDBACK_LIFETIME {
                continue;
            }
            let target = GameObjectRef::new(hit.target).game_object(scene);
            let at = target.map_or(hit.position, |target| {
                scene.world_transform(target).position
            });
            if !target.is_some_and(|target| crate::vision::is_friendly(scene, player, target))
                && !map.vision.visible(vec2(at.x, at.z))
            {
                continue;
            }
            let bounds = target.and_then(|target| projection.visual_bounds(scene, target));
            let anchor = bounds
                .map(hp_anchor)
                .map(|(x, y, w)| (x - w * 0.4, y - 25.0))
                .or_else(|| projection.point(hit.position).map(|(x, y)| (x, y - 70.0)));
            let Some((x, base_y)) = anchor else {
                continue;
            };
            // New hits are appended; each newer hit sits above the surviving older hits.
            let stack = stats.feedback[..index]
                .iter()
                .filter(|older| {
                    older.target == hit.target
                        && older.age < crate::combat::DAMAGE_FEEDBACK_LIFETIME
                })
                .count();
            let tier = damage_tier(
                hit.amount,
                hit.max_health,
                hud.low_damage_ratio,
                hud.high_damage_ratio,
            );
            let (size, color) = match tier {
                DamageTier::High => (35.0, RED),
                DamageTier::Normal => (28.0, LIGHT),
                DamageTier::Low => (23.0, MUTED),
            };
            let pop = 1.0 + 0.22 * (1.0 - hit.age / 0.14).clamp(0.0, 1.0);
            let opacity =
                ((crate::combat::DAMAGE_FEEDBACK_LIFETIME - hit.age) / 0.35).clamp(0.0, 1.0);
            let number = if hit.amount < 1.0 {
                format!("-{:.1}", hit.amount)
            } else {
                format!("-{:.0}", hit.amount)
            };
            let number = if tier == DamageTier::High {
                format!("{number}!")
            } else {
                number
            };
            p.label(
                x,
                base_y - stack as f32 * 35.0 - hit.age * 10.0,
                number,
                size,
                color.with_alpha(opacity),
                calyx_engine::ui::TextAnchor::CenterRight,
                None,
                true,
                Some(3.0),
            );
            if let Some(PaintCommand::Label(label)) = p.commands.last_mut() {
                label.scale = pop;
            }
        }
        if let Some(hit) = stats
            .feedback
            .iter()
            .filter(|h| h.age < 0.25)
            .min_by(|a, b| a.age.total_cmp(&b.age))
        {
            if let Some((x, y)) = aim {
                let scale = 1.0 + 0.25 * (1.0 - hit.age / 0.1).clamp(0.0, 1.0);
                let opacity = ((0.25 - hit.age) / 0.1).clamp(0.0, 1.0);
                for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let points = |outer: f32, inner: f32, thick: f32| {
                        [
                            (x + dx * inner * scale, y + dy * inner * scale),
                            (x + dx * outer * scale, y + dy * (outer - thick) * scale),
                            (
                                x + dx * (outer + 2.0) * scale,
                                y + dy * (outer + 2.0) * scale,
                            ),
                            (x + dx * (outer - thick) * scale, y + dy * outer * scale),
                        ]
                    };
                    p.polygon(&points(32.0, 8.0, 10.0), BLACK.with_alpha(opacity));
                    p.polygon(&points(29.0, 11.0, 6.0), LIGHT.with_alpha(opacity));
                }
            }
        }
    });
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
    fn timer_and_radar_remain_without_removed_bottom_instrument_panels() {
        let (scene, _) = setup();
        for (width, height) in [(1672.0, 941.0), (960.0, 540.0)] {
            let commands = commands(&scene, width, height);
            let text = commands
                .iter()
                .filter_map(|c| {
                    if let PaintCommand::Label(label) = c {
                        Some(label.text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert!(text.contains(&"05:00"));
            assert!(text.contains(&"N"));
            assert!(!text
                .iter()
                .any(|s| s.contains("HULL") || s.contains("CANNON") || s.contains("RELOAD")));
        }
    }

    #[test]
    fn health_bar_width_is_fixed_and_anchor_clears_projected_top() {
        let far = hp_anchor(ScreenBounds {
            left: 100.0,
            right: 180.0,
            top: 200.0,
        });
        let near = hp_anchor(ScreenBounds {
            left: 100.0,
            right: 260.0,
            top: 200.0,
        });
        let tiny = hp_anchor(ScreenBounds {
            left: 0.0,
            right: 10.0,
            top: 80.0,
        });
        assert_eq!(far, (140.0, 192.0, HP_BAR_WIDTH));
        assert_eq!(near, (180.0, 192.0, HP_BAR_WIDTH));
        assert_eq!(tiny, (5.0, 72.0, HP_BAR_WIDTH));
    }

    /// The outer frame of the one world health bar in a HUD frame: a black
    /// fill directly followed by its panel fill and its friendly cyan fill.
    fn health_bar_frame(commands: &[PaintCommand]) -> UiRect {
        let frames = commands
            .windows(3)
            .filter_map(|window| match window {
                [PaintCommand::FillRect {
                    rect, color: BLACK, ..
                }, PaintCommand::FillRect {
                    color: paint::PANEL,
                    ..
                }, PaintCommand::FillRect { color: CYAN, .. }] => Some(*rect),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 1, "expected one health bar");
        frames[0]
    }

    #[test]
    fn turning_a_tank_in_place_keeps_its_health_bar_size() {
        use calyx_engine::assets::{mesh::Mesh, AssetRef};
        use calyx_engine::component::{ComponentCamera, ComponentMesh};
        use calyx_engine::math::Transform;
        use nalgebra::UnitQuaternion;
        use nalgebra_glm::vec3;

        let assets = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
        let mut hull = Mesh::new(&assets.render_context);
        // A tank-shaped hull: longer than it is wide, so its projected width
        // changes as it turns.
        hull.vertices = vec![vec3(-1.1, 0.0, -1.7), vec3(1.1, 1.3, 1.7)];
        let hull = assets
            .registries
            .assets
            .read()
            .create("hud-tank-hull".into(), hull)
            .unwrap();
        let mut scene = assets.registries.lock_read().scene();
        let tank = scene.create(None, None);
        scene.add_component(tank, ComponentTankController::default());
        scene.add_component(
            tank,
            ComponentHealth {
                current_health: 60.0,
                max_health: 100.0,
                ..Default::default()
            },
        );
        scene.add_component(tank, ComponentTankHud::default());
        scene.add_component(
            tank,
            ComponentMesh {
                mesh: AssetRef::from_id(hull.id()),
                ..Default::default()
            },
        );
        // The arena's chase camera, relative to the tank.
        let camera = scene.create(None, None);
        scene.add_component(
            camera,
            ComponentCamera {
                fov: 1.117,
                ..Default::default()
            },
        );
        scene.set_transform(
            camera,
            &Transform::from_components(
                vec3(0.0, 34.0, -13.7),
                UnitQuaternion::from_euler_angles(68.05f32.to_radians(), 0.0, 0.0),
                Vec3::repeat(1.0),
            )
            .matrix(),
        );
        let (width, height) = (1326.0, 746.0);
        let projection = Projection::scene(&scene, width, height).unwrap();
        let mut bounds = Vec::new();
        let mut frames = Vec::new();
        for degrees in [0.0f32, 40.0, 90.0, 135.0] {
            scene.set_transform(
                tank,
                &Transform::from_components(
                    Vec3::zeros(),
                    UnitQuaternion::from_euler_angles(0.0, degrees.to_radians(), 0.0),
                    Vec3::repeat(1.0),
                )
                .matrix(),
            );
            let projected = projection.visual_bounds(&scene, tank).unwrap();
            bounds.push(projected.right - projected.left);
            frames.push(health_bar_frame(&commands(&scene, width, height)));
        }
        // The projected bounds really do change as the tank turns ...
        let (narrow, wide) = bounds.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), w| {
            (lo.min(*w), hi.max(*w))
        });
        assert!(wide > narrow * 1.2, "bounds widths {bounds:?}");
        // ... but the bar keeps one width and thickness.
        for frame in &frames {
            assert!(
                (frame.width() - (HP_BAR_WIDTH + 6.0)).abs() < 1e-3,
                "{frames:?}"
            );
            assert!(
                (frame.height() - (HP_BAR_THICKNESS + 6.0)).abs() < 1e-3,
                "{frames:?}"
            );
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
            serde_json::from_value(serde_json::to_value(&hud).unwrap()).unwrap();
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
    fn damage_tiers_are_removed_health_ratios() {
        assert_eq!(damage_tier(10.0, 100.0, 0.15, 0.5), DamageTier::Low);
        assert_eq!(damage_tier(35.0, 100.0, 0.15, 0.5), DamageTier::Normal);
        assert_eq!(damage_tier(60.0, 100.0, 0.15, 0.5), DamageTier::High);
    }

    #[test]
    fn scoreboard_is_held_and_bounded_with_many_real_rows() {
        let (mut scene, tank) = setup();
        for i in 0..80 {
            let actor = scene.create(None, None);
            scene.add_component(actor, ComponentTankController::default());
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
            .any(|c| matches!(c,PaintCommand::Label(label)if label.text=="SCOREBOARD")));
        scene.write_component::<ComponentTankHud, _>(tank, |hud| hud.scoreboard_open = true);
        let commands = commands(&scene, 960.0, 540.0);
        assert!(commands
            .iter()
            .any(|c| matches!(c,PaintCommand::Label(label)if label.text=="SCOREBOARD")));
        assert!(commands
            .iter()
            .any(|c| matches!(c,PaintCommand::Label(label)if label.text.starts_with("PAGE 1/"))));
        for command in commands {
            if let PaintCommand::Label(label) = command {
                assert!(label.position.y <= 590.0, "text overflow {label:?}");
            }
        }
    }

    #[test]
    fn scoreboard_blur_is_local_reused_and_removed_without_touching_world_effects() {
        let (mut scene, owner) = setup();
        let camera = scene.create(None, None);
        scene.add_component(camera, calyx_engine::component::ComponentCamera::default());
        let world = scene.create(None, None);
        scene.add_component(
            world,
            calyx_engine::component::ComponentGlobalPostProcessVolume::default(),
        );
        let authored = ComponentOutline {
            styles: vec![calyx_engine::component::OutlineStyle {
                opacity: 0.85,
                ..Default::default()
            }],
            ..Default::default()
        };
        scene.add_component(world, authored.clone());
        let mut settings = calyx_engine::render::postprocess::PostProcessSettings::default();
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
        settings.resolve_into(&scene, &Vec3::zeros());
        assert_eq!(
            settings.get::<ComponentOutline>().unwrap().styles[0].opacity,
            0.0
        );
        assert_eq!(
            settings.get::<ComponentDepthOfField>().unwrap().max_blur,
            30.0
        );
        let reused = update_scoreboard_blur(&mut scene, owner, true, blur);
        assert_eq!(reused.game_object(&scene), Some(object));
        update_scoreboard_blur(&mut scene, owner, false, blur);
        scene.flush_deletes();
        assert_eq!(scene.objects().count(), before);
        settings.resolve_into(&scene, &Vec3::zeros());
        assert_eq!(settings.get::<ComponentOutline>(), Some(&authored));
        assert!(settings.get::<ComponentDepthOfField>().is_none());
        let blur = update_scoreboard_blur(&mut scene, owner, true, Default::default());
        scene.delete(owner);
        scene.flush_deletes();
        assert!(blur.game_object(&scene).is_none());
    }
}
