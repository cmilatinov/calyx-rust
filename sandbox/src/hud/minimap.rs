//! A top-down map of the arena, drawn from world positions each frame.

use super::{hud_text, HudStyle};
use crate::arena::ComponentArena;
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
use engine::scene::{GameObject, Scene};
use engine::ui::{
    CornerRadius, EdgeInsets, LayoutNode, PaintCommand, PointerEvents, Style, UiArena, UiColor,
    UiLength, UiNodeHandle, UiPoint, UiRect, UiSize, UiWidget, Widget,
};
use nalgebra_glm::{vec2, Vec2, Vec3};

/// Half the side of the square the map shows when the scene has no arena,
/// in metres, centred on the player.
const UNBOUNDED_HALF_EXTENT: f32 = 25.0;

/// Which part of the ground the map shows, and which way it faces.
///
/// Ground positions are `(x, z)`. `right` and `up` are the unit ground
/// directions that point right and up on the map, and `half_extent` is how
/// far the map reaches from `center` along each of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MinimapView {
    pub center: Vec2,
    pub half_extent: Vec2,
    pub right: Vec2,
    pub up: Vec2,
}

impl MinimapView {
    /// A view whose map right is `right`, a unit ground direction; map up is
    /// the ground direction a quarter turn from it.
    pub fn facing(center: Vec2, half_extent: Vec2, right: Vec2) -> Self {
        Self {
            center,
            half_extent,
            right,
            up: vec2(-right.y, right.x),
        }
    }

    /// A view that fits an axis-aligned ground rectangle of `size` (along X
    /// and Z) centred on `center`.
    pub fn fitting(center: Vec2, size: Vec2, right: Vec2) -> Self {
        let mut view = Self::facing(center, Vec2::zeros(), right);
        let half = size * 0.5;
        view.half_extent = vec2(
            view.right.x.abs() * half.x + view.right.y.abs() * half.y,
            view.up.x.abs() * half.x + view.up.y.abs() * half.y,
        );
        view
    }

    /// Map pixels per metre for a map drawn in `rect`; the map keeps its
    /// aspect and fits the whole view.
    pub fn scale(&self, rect: UiRect) -> f32 {
        let x = rect.width() / (2.0 * self.half_extent.x).max(f32::EPSILON);
        let y = rect.height() / (2.0 * self.half_extent.y).max(f32::EPSILON);
        x.min(y)
    }

    /// Where the ground position `world` falls on a map drawn in `rect`.
    pub fn map_point(&self, world: Vec2, rect: UiRect) -> UiPoint {
        let offset = world - self.center;
        let scale = self.scale(rect);
        let center = rect_center(rect);
        UiPoint::new(
            center.x + offset.dot(&self.right) * scale,
            center.y - offset.dot(&self.up) * scale,
        )
    }

    /// The ground direction `direction` as a map direction, y down.
    pub fn map_direction(&self, direction: Vec2) -> Vec2 {
        vec2(direction.dot(&self.right), -direction.dot(&self.up))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlipKind {
    Player,
    Tank,
    Sentry,
    Target,
}

/// Something the map marks: where it is on the ground and, for tanks, the
/// ground direction its hull faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MinimapBlip {
    pub kind: BlipKind,
    pub position: Vec2,
    pub heading: Option<Vec2>,
}

/// Everything the map draws, read from the scene.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MinimapState {
    pub view: MinimapView,
    /// The arena floor's ground size, walls included, when there is one.
    pub arena_size: Option<Vec2>,
    /// Centre and side of each wall-height block of cover.
    pub cover: Vec<(Vec2, f32)>,
    pub blips: Vec<MinimapBlip>,
}

/// Reads the map of the scene `player`'s HUD shows: the first arena, facing
/// the way `player`'s camera does, and every tank, sentry and live target.
pub(crate) fn minimap_state(scene: &Scene, player: GameObject) -> MinimapState {
    let right = camera_right(scene, player).unwrap_or_else(|| vec2(1.0, 0.0));
    let arena = scene.objects().find_map(|object| {
        scene
            .read_component::<ComponentArena, _, _>(object, |arena| {
                Some((arena.extent()?, arena.cell_size, arena.cover_positions()))
            })
            .flatten()
            .map(|arena| (scene.world_transform(object), arena))
    });

    let (view, arena_size, cover) = match arena {
        Some((transform, ((width, depth), cell_size, cover))) => {
            let center = ground(transform.position);
            let size = vec2(width, depth);
            let cover = cover
                .iter()
                .map(|position| (ground(transform.transform_position(position)), cell_size))
                .collect();
            (MinimapView::fitting(center, size, right), Some(size), cover)
        }
        None => (
            MinimapView::facing(
                ground(scene.world_transform(player).position),
                vec2(UNBOUNDED_HALF_EXTENT, UNBOUNDED_HALF_EXTENT),
                right,
            ),
            None,
            Vec::new(),
        ),
    };

    MinimapState {
        view,
        arena_size,
        cover,
        blips: scene
            .objects()
            .filter_map(|object| blip(scene, player, object))
            .collect(),
    }
}

fn blip(scene: &Scene, player: GameObject, object: GameObject) -> Option<MinimapBlip> {
    let has = |present: Option<()>| present.is_some();
    let is_tank = has(scene.read_component::<ComponentTankController, _, _>(object, |_| ()));
    let kind = if is_tank {
        let alive = scene
            .read_component::<ComponentRespawnState, _, _>(object, |state| state.alive)
            .unwrap_or(true);
        if !alive {
            return None;
        }
        if object == player {
            BlipKind::Player
        } else {
            BlipKind::Tank
        }
    } else if has(scene.read_component::<ComponentSentry, _, _>(object, |_| ())) {
        BlipKind::Sentry
    } else if has(scene.read_component::<ComponentProjectileTarget, _, _>(object, |_| ())) {
        let dead = scene
            .read_component::<ComponentHealth, _, _>(object, |health| health.dead)
            .unwrap_or(false);
        if dead {
            return None;
        }
        BlipKind::Target
    } else {
        return None;
    };

    let transform = scene.world_transform(object);
    Some(MinimapBlip {
        kind,
        position: ground(transform.position),
        heading: is_tank
            .then(|| ground(transform.forward()).try_normalize(f32::EPSILON))
            .flatten(),
    })
}

/// The ground direction that points right in `player`'s camera view.
fn camera_right(scene: &Scene, player: GameObject) -> Option<Vec2> {
    let camera = scene
        .read_component::<ComponentTankController, _, _>(player, |controller| controller.camera)?
        .game_object(scene)?;
    ground(scene.world_transform(camera).right()).try_normalize(f32::EPSILON)
}

fn ground(position: Vec3) -> Vec2 {
    vec2(position.x, position.z)
}

fn rect_center(rect: UiRect) -> UiPoint {
    UiPoint::new(
        (rect.min.x + rect.max.x) * 0.5,
        (rect.min.y + rect.max.y) * 0.5,
    )
}

/// The minimap panel: a label over the map.
pub(super) struct MinimapHud {
    pub style: HudStyle,
    pub state: MinimapState,
    pub size: UiSize,
    pub margin: EdgeInsets,
}

impl Widget for MinimapHud {
    fn build(&self, ui: &mut UiArena) -> UiNodeHandle {
        let label = hud_text(
            ui,
            "hud-minimap-label",
            "TACTICAL MAP",
            self.style.label_style(self.style.muted),
        );
        let map = ui
            .alloc_node(MinimapWidget {
                state: self.state.clone(),
                colors: MinimapColors::from(self.style),
            })
            .id(ui, "hud-minimap-map")
            .background(ui, self.style.bar_background)
            .border(ui, engine::ui::Border::solid(self.style.outline, 1.0))
            .radius(ui, CornerRadius::all(2.0))
            .width(ui, UiLength::Fill)
            .flex_grow(ui, 1.0)
            .pointer_events(ui, PointerEvents::None);
        ui.column()
            .id(ui, "hud-minimap")
            .style(ui, self.style.panel_style())
            .width(ui, UiLength::Px(self.size.width))
            .height(ui, UiLength::Px(self.size.height))
            .margin(ui, self.margin)
            .child(ui, label)
            .child(ui, map)
    }
}

#[derive(Clone, Copy)]
struct MinimapColors {
    floor: UiColor,
    bounds: UiColor,
    cover: UiColor,
    player: UiColor,
    heading: UiColor,
    enemy: UiColor,
    target: UiColor,
}

impl From<HudStyle> for MinimapColors {
    fn from(style: HudStyle) -> Self {
        Self {
            floor: UiColor::rgba(52, 56, 54, 200),
            bounds: style.outline,
            cover: UiColor::rgba(110, 116, 114, 220),
            player: style.health,
            heading: style.text,
            enemy: UiColor::rgba(226, 88, 72, 245),
            target: style.ammo,
        }
    }
}

struct MinimapWidget {
    state: MinimapState,
    colors: MinimapColors,
}

impl MinimapWidget {
    fn ground_rect(&self, center: Vec2, size: Vec2, rect: UiRect) -> UiRect {
        let view = &self.state.view;
        let half = size * 0.5;
        let a = view.map_point(center - half, rect);
        let b = view.map_point(center + half, rect);
        UiRect {
            min: UiPoint::new(a.x.min(b.x), a.y.min(b.y)),
            max: UiPoint::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }
}

impl UiWidget for MinimapWidget {
    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        let rect = node.content_rect.inset(EdgeInsets::all(4.0));
        let view = &self.state.view;
        let colors = self.colors;

        if let Some(size) = self.state.arena_size {
            let floor = self.ground_rect(view.center, size, rect);
            commands.push(PaintCommand::FillRect {
                rect: floor,
                color: colors.floor,
                radius: CornerRadius::default(),
            });
            commands.push(PaintCommand::StrokeRect {
                rect: floor,
                border: engine::ui::Border::solid(colors.bounds, 1.5),
                radius: CornerRadius::default(),
            });
        }
        for &(center, side) in &self.state.cover {
            commands.push(PaintCommand::FillRect {
                rect: self.ground_rect(center, vec2(side, side), rect),
                color: colors.cover,
                radius: CornerRadius::default(),
            });
        }

        // The player draws last, over everything else.
        let mut blips: Vec<&MinimapBlip> = self.state.blips.iter().collect();
        blips.sort_by_key(|blip| blip.kind == BlipKind::Player);
        for blip in blips {
            let center = view.map_point(blip.position, rect);
            let (color, radius, round) = match blip.kind {
                BlipKind::Player => (colors.player, 4.5, true),
                BlipKind::Tank => (colors.enemy, 4.0, true),
                BlipKind::Sentry => (colors.enemy, 3.5, false),
                BlipKind::Target => (colors.target, 3.5, true),
            };
            if let Some(heading) = blip.heading {
                let direction = view.map_direction(heading);
                commands.push(PaintCommand::Line {
                    start: center,
                    end: UiPoint::new(
                        center.x + direction.x * radius * 2.6,
                        center.y + direction.y * radius * 2.6,
                    ),
                    color: colors.heading,
                    width: 2.0,
                });
            }
            commands.push(PaintCommand::FillRect {
                rect: UiRect {
                    min: UiPoint::new(center.x - radius, center.y - radius),
                    max: UiPoint::new(center.x + radius, center.y + radius),
                },
                color,
                radius: if round {
                    CornerRadius::all(radius)
                } else {
                    CornerRadius::all(1.0)
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{minimap_state, BlipKind, MinimapView};
    use crate::arena::ComponentArena;
    use crate::sentry::ComponentSentry;
    use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
    use engine::component::ComponentID;
    use engine::math::Transform;
    use engine::scene::GameObjectRef;
    use engine::ui::{UiPoint, UiRect, UiSize};
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec2, vec3};

    fn map_rect() -> UiRect {
        UiRect::from_min_size(UiPoint::new(100.0, 50.0), UiSize::new(200.0, 200.0))
    }

    fn assert_near(actual: UiPoint, expected: UiPoint) {
        assert!(
            (actual.x - expected.x).abs() < 1e-3 && (actual.y - expected.y).abs() < 1e-3,
            "{actual:?} should be {expected:?}"
        );
    }

    #[test]
    fn an_arena_fit_view_maps_its_corners_to_the_map_edges() {
        // A 40 x 20 arena centred on (0, 10), with +X to the right.
        let view = MinimapView::fitting(vec2(0.0, 10.0), vec2(40.0, 20.0), vec2(1.0, 0.0));

        // 200 px across 40 m: 5 px per metre, so the 20 m depth spans 100 px
        // around the map centre (200, 150).
        assert_eq!(view.scale(map_rect()), 5.0);
        assert_near(
            view.map_point(vec2(0.0, 10.0), map_rect()),
            UiPoint::new(200.0, 150.0),
        );
        assert_near(
            view.map_point(vec2(-20.0, 0.0), map_rect()),
            UiPoint::new(100.0, 200.0),
        );
        assert_near(
            view.map_point(vec2(20.0, 20.0), map_rect()),
            UiPoint::new(300.0, 100.0),
        );
    }

    #[test]
    fn a_turned_view_keeps_the_camera_right_on_the_right() {
        // The camera looks down -X, so its right is +Z and map up is -X.
        let view = MinimapView::fitting(vec2(0.0, 0.0), vec2(40.0, 20.0), vec2(0.0, 1.0));

        assert_eq!(view.up, vec2(-1.0, 0.0));
        assert_eq!(view.half_extent, vec2(10.0, 20.0));
        assert_near(
            view.map_point(vec2(0.0, 10.0), map_rect()),
            UiPoint::new(250.0, 150.0),
        );
        assert_near(
            view.map_point(vec2(-20.0, 0.0), map_rect()),
            UiPoint::new(200.0, 50.0),
        );
        assert_eq!(view.map_direction(vec2(-1.0, 0.0)), vec2(0.0, -1.0));
    }

    #[test]
    fn minimap_state_marks_tanks_sentries_and_live_targets() {
        let mut scene = engine::test_support::test_scene();
        let named = |scene: &mut engine::scene::Scene, name: &str| {
            scene.create(
                Some(ComponentID {
                    name: name.to_string(),
                    ..Default::default()
                }),
                None,
            )
        };
        let arena = named(&mut scene, "Arena");
        scene.set_world_transform(arena, Transform::from_xyz(0.0, 0.0, 11.0).matrix());
        scene.add_component(arena, ComponentArena::default());
        let camera = named(&mut scene, "Camera");
        let player = named(&mut scene, "Tank");
        scene.set_world_transform(
            player,
            Transform::from_components(
                vec3(2.0, 0.0, 4.0),
                UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        scene.add_component(
            player,
            ComponentTankController {
                camera: GameObjectRef::new(scene.uuid(camera)),
                ..Default::default()
            },
        );
        let sentry = named(&mut scene, "Sentry");
        scene.add_component(sentry, ComponentSentry::default());
        for (name, dead) in [("Target A", false), ("Target B", true)] {
            let target = named(&mut scene, name);
            scene.add_component(target, ComponentProjectileTarget::default());
            scene.add_component(
                target,
                ComponentHealth {
                    dead,
                    ..Default::default()
                },
            );
        }

        let state = minimap_state(&scene, player);

        // The default 15 x 16 arena of 2 m cells, centred where it stands.
        assert_eq!(state.arena_size, Some(vec2(30.0, 32.0)));
        assert_eq!(state.view.center, vec2(0.0, 11.0));
        assert_eq!(state.cover.len(), 9);
        let mut kinds: Vec<BlipKind> = state.blips.iter().map(|blip| blip.kind).collect();
        kinds.sort_by_key(|kind| *kind as u8);
        assert_eq!(
            kinds,
            vec![BlipKind::Player, BlipKind::Sentry, BlipKind::Target]
        );
        let player_blip = state
            .blips
            .iter()
            .find(|blip| blip.kind == BlipKind::Player)
            .unwrap();
        assert_eq!(player_blip.position, vec2(2.0, 4.0));
        let heading = player_blip.heading.expect("a tank shows its heading");
        assert!((heading - vec2(1.0, 0.0)).norm() < 1e-5, "{heading:?}");
    }
}
