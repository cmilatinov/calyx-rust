//! A top-down map of the arena, drawn from world positions each frame.

use super::{hud_text, HudStyle};
use crate::arena::ComponentArena;
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
use engine::math::Transform;
use engine::scene::{GameObject, Scene};
use engine::ui::{
    CornerRadius, EdgeInsets, LayoutNode, PaintCommand, PointerEvents, Style, StylePatch, UiArena,
    UiColor, UiLength, UiNodeHandle, UiPoint, UiRect, UiSize, UiTransform, UiWidget, Widget,
};
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

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

    /// A view centred on the ground rectangle `area` that fits all of it.
    pub fn fitting(area: GroundRect, right: Vec2) -> Self {
        let mut view = Self::facing(area.center, Vec2::zeros(), right);
        let [a, b] = area.half_axes;
        view.half_extent = vec2(
            a.dot(&view.right).abs() + b.dot(&view.right).abs(),
            a.dot(&view.up).abs() + b.dot(&view.up).abs(),
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

    /// The ground rectangle `area` on a map drawn in `rect`: a screen
    /// rectangle and the turn about its centre that lays it over `area`.
    pub fn map_rect(&self, area: GroundRect, rect: UiRect) -> (UiRect, UiTransform) {
        let scale = self.scale(rect);
        let center = self.map_point(area.center, rect);
        let a = self.map_direction(area.half_axes[0]) * scale;
        let b = self.map_direction(area.half_axes[1]) * scale;
        let (half_width, half_height) = (a.norm(), b.norm());
        let screen = UiRect {
            min: UiPoint::new(center.x - half_width, center.y - half_height),
            max: UiPoint::new(center.x + half_width, center.y + half_height),
        };
        let turn = UiTransform {
            rotate_z: if half_width > f32::EPSILON {
                a.y.atan2(a.x)
            } else {
                0.0
            },
            ..UiTransform::identity()
        };
        (screen, turn)
    }
}

/// A rectangle on the ground: its centre and the two half-axes from the
/// centre to the middle of two adjacent sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GroundRect {
    pub center: Vec2,
    pub half_axes: [Vec2; 2],
}

impl GroundRect {
    /// A rectangle of `size` along X and Z, centred on `center`.
    #[cfg(test)]
    pub fn axis_aligned(center: Vec2, size: Vec2) -> Self {
        Self {
            center,
            half_axes: [vec2(size.x * 0.5, 0.0), vec2(0.0, size.y * 0.5)],
        }
    }

    /// The ground under a rectangle of `size` along its local X and Z,
    /// centred on `center` in the local space of `transform`.
    pub fn placed(transform: &Transform, center: Vec3, size: Vec2) -> Self {
        let half_axis = |local: Vec3| ground(transform.transform_direction(&local));
        Self {
            center: ground(transform.transform_position(&center)),
            half_axes: [
                half_axis(vec3(size.x * 0.5, 0.0, 0.0)),
                half_axis(vec3(0.0, 0.0, size.y * 0.5)),
            ],
        }
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
    /// The arena floor, walls included, when there is one.
    pub floor: Option<GroundRect>,
    /// Each wall-height block of cover.
    pub cover: Vec<GroundRect>,
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

    let (view, floor, cover) = match arena {
        Some((transform, ((width, depth), cell_size, cover))) => {
            let floor = GroundRect::placed(&transform, Vec3::zeros(), vec2(width, depth));
            let cover = cover
                .iter()
                .map(|position| {
                    GroundRect::placed(&transform, *position, vec2(cell_size, cell_size))
                })
                .collect();
            (MinimapView::fitting(floor, right), Some(floor), cover)
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
        floor,
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
            .pointer_events(ui, PointerEvents::None)
            // Blips past the view's reach stay inside the panel.
            .style(ui, StylePatch::default().clip(true));
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

impl UiWidget for MinimapWidget {
    fn default_size(&self, _style: &Style, max: UiSize) -> UiSize {
        max
    }

    fn paint(&self, node: &LayoutNode, commands: &mut Vec<PaintCommand>) {
        self.paint_map(node.content_rect.inset(EdgeInsets::all(4.0)), commands);
    }
}

impl MinimapWidget {
    /// Paints the map into `rect`.
    fn paint_map(&self, rect: UiRect, commands: &mut Vec<PaintCommand>) {
        let view = &self.state.view;
        let colors = self.colors;

        if let Some(floor) = self.state.floor {
            let (floor, turn) = view.map_rect(floor, rect);
            turned(commands, floor, turn, |commands| {
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
            });
        }
        for &cover in &self.state.cover {
            let (cover, turn) = view.map_rect(cover, rect);
            turned(commands, cover, turn, |commands| {
                commands.push(PaintCommand::FillRect {
                    rect: cover,
                    color: colors.cover,
                    radius: CornerRadius::default(),
                });
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

/// Paints what `paint` pushes turned by `turn` about the centre of `rect`.
fn turned(
    commands: &mut Vec<PaintCommand>,
    rect: UiRect,
    turn: UiTransform,
    paint: impl FnOnce(&mut Vec<PaintCommand>),
) {
    if turn.is_identity() {
        paint(commands);
        return;
    }
    commands.push(PaintCommand::PushTransform {
        rect,
        transform: turn,
    });
    paint(commands);
    commands.push(PaintCommand::PopTransform);
}

#[cfg(test)]
mod tests {
    use super::{
        minimap_state, BlipKind, GroundRect, MinimapColors, MinimapState, MinimapView,
        MinimapWidget,
    };
    use crate::arena::ComponentArena;
    use crate::hud::HudStyle;
    use crate::sentry::ComponentSentry;
    use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
    use engine::component::ComponentID;
    use engine::math::Transform;
    use engine::scene::GameObjectRef;
    use engine::ui::{PaintCommand, UiColor, UiTransform};
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
        let view = MinimapView::fitting(
            GroundRect::axis_aligned(vec2(0.0, 10.0), vec2(40.0, 20.0)),
            vec2(1.0, 0.0),
        );

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
        let view = MinimapView::fitting(
            GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(40.0, 20.0)),
            vec2(0.0, 1.0),
        );

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
        assert_eq!(
            state.floor,
            Some(GroundRect::axis_aligned(vec2(0.0, 11.0), vec2(30.0, 32.0)))
        );
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

    /// Every filled rectangle `commands` paint, as the four screen corners
    /// it covers once the active transforms rotate it.
    fn painted_quads(commands: &[PaintCommand]) -> Vec<(UiColor, [UiPoint; 4])> {
        let rotate = |point: UiPoint, rect: UiRect, transform: UiTransform| {
            assert_eq!((transform.rotate_x, transform.rotate_y), (0.0, 0.0));
            let center = UiPoint::new(
                (rect.min.x + rect.max.x) * 0.5,
                (rect.min.y + rect.max.y) * 0.5,
            );
            let (x, y) = (
                (point.x - center.x) * transform.scale,
                (point.y - center.y) * transform.scale,
            );
            let (sin, cos) = transform.rotate_z.sin_cos();
            UiPoint::new(center.x + x * cos - y * sin, center.y + x * sin + y * cos)
        };
        let mut transforms: Vec<(UiRect, UiTransform)> = Vec::new();
        let mut quads = Vec::new();
        for command in commands {
            match command {
                PaintCommand::PushTransform { rect, transform } => {
                    transforms.push((*rect, *transform))
                }
                PaintCommand::PopTransform => {
                    transforms.pop();
                }
                PaintCommand::FillRect { rect, color, .. } => {
                    let corners = [
                        rect.min,
                        UiPoint::new(rect.max.x, rect.min.y),
                        rect.max,
                        UiPoint::new(rect.min.x, rect.max.y),
                    ]
                    .map(|corner| {
                        transforms
                            .iter()
                            .rev()
                            .fold(corner, |corner, (rect, transform)| {
                                rotate(corner, *rect, *transform)
                            })
                    });
                    quads.push((*color, corners));
                }
                _ => {}
            }
        }
        quads
    }

    fn assert_same_corners(actual: [UiPoint; 4], expected: [UiPoint; 4]) {
        for corner in expected {
            assert!(
                actual
                    .iter()
                    .any(|point| (point.x - corner.x).abs() < 1e-3
                        && (point.y - corner.y).abs() < 1e-3),
                "{corner:?} should be a corner of {actual:?}"
            );
        }
    }

    #[test]
    fn a_diagonal_view_paints_the_arena_floor_as_a_rotated_quad() {
        // The camera's right is 45 degrees between +X and +Z.
        let right = vec2(1.0, 1.0).normalize();
        let floor = GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(30.0, 32.0));
        let view = MinimapView::fitting(floor, right);
        let colors = MinimapColors::from(HudStyle::default());
        let widget = MinimapWidget {
            state: MinimapState {
                view,
                floor: Some(floor),
                cover: Vec::new(),
                blips: Vec::new(),
            },
            colors,
        };
        let mut commands = Vec::new();

        widget.paint_map(map_rect(), &mut commands);

        let floor = painted_quads(&commands)
            .into_iter()
            .find(|(color, _)| *color == colors.floor)
            .expect("the floor is painted")
            .1;
        let expected = [
            vec2(-15.0, -16.0),
            vec2(15.0, -16.0),
            vec2(15.0, 16.0),
            vec2(-15.0, 16.0),
        ]
        .map(|corner| view.map_point(corner, map_rect()));
        assert_same_corners(floor, expected);
    }

    #[test]
    fn minimap_state_reads_the_arena_at_its_world_scale() {
        let mut scene = engine::test_support::test_scene();
        let arena = scene.create(None, None);
        // Twice as wide along X, and turned a quarter about Y so that its
        // local X runs along world -Z.
        scene.set_world_transform(
            arena,
            Transform::from_components(
                vec3(0.0, 0.0, 11.0),
                UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
                vec3(2.0, 1.0, 1.0),
            )
            .matrix(),
        );
        scene.add_component(arena, ComponentArena::default());
        let player = scene.create(None, None);

        let state = minimap_state(&scene, player);

        // The default 30 x 32 arena, stretched to 60 m along its local X.
        let floor = state.floor.expect("the arena has a floor");
        assert!((floor.center - vec2(0.0, 11.0)).norm() < 1e-4, "{floor:?}");
        assert!(
            (floor.half_axes[0] - vec2(0.0, -30.0)).norm() < 1e-4,
            "{floor:?}"
        );
        assert!(
            (floor.half_axes[1] - vec2(16.0, 0.0)).norm() < 1e-4,
            "{floor:?}"
        );
        assert!(
            (state.view.half_extent - vec2(16.0, 30.0)).norm() < 1e-4,
            "{:?}",
            state.view
        );
        // Each 2 m block of cover is 4 m along the arena's local X.
        for cover in &state.cover {
            assert!((cover.half_axes[0].norm() - 2.0).abs() < 1e-4, "{cover:?}");
            assert!((cover.half_axes[1].norm() - 1.0).abs() < 1e-4, "{cover:?}");
        }
    }
}
