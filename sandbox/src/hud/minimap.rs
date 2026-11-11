//! A top-down map of the arena, drawn from world positions each frame.

use super::paint::{Painter, BLACK, CYAN, LIGHT, MUTED, RED, STEEL};
use crate::combat::same_side;
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
#[cfg(test)]
use crate::vision::arena_geometry;
#[cfg(test)]
use crate::vision::cover_entry;
use crate::vision::sight::{side_sight, sight_rays};
use crate::vision::{ground, GroundRect, SightRays, TeamVision, VisionFrame};
use calyx_engine::component::FogOfWarSight;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{UiColor, UiPoint, UiRect, UiSize};
use calyx_engine::utils::Uuid;
use nalgebra_glm::{vec2, Vec2};

/// Half the side of the square the map shows when the scene has no arena,
/// in metres, centred on the player.
const UNBOUNDED_HALF_EXTENT: f32 = 25.0;
/// How far the map reaches from the player, in multiples of the player's
/// vision range: far enough to show the arena past what the main view
/// already shows.
const MAP_REACH: f32 = 3.0;

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
    pub friendly: bool,
    /// The object the blip marks.
    pub id: Uuid,
    /// How present fog of war leaves the object, from 0 to 1, as its
    /// [`crate::vision::FogState::presence`]; `None` when fog has not looked
    /// at it, and the map's vision decides alone.
    pub presence: Option<f32>,
}

/// Everything the map draws, read from the scene.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MinimapState {
    pub view: MinimapView,
    /// The arena floor, walls included, when there is one and its
    /// rectangle is the playable area's edge; an arena bounded by a border
    /// of another shape has none, and the border's cover outlines it.
    pub floor: Option<GroundRect>,
    /// Each wall-height block of cover.
    pub cover: Vec<GroundRect>,
    pub blips: Vec<MinimapBlip>,
    pub player_position: Vec2,
    /// How far the player sees, which sets how far the map reaches.
    pub vision_range: f32,
    /// What the player's side sees; the main view hides and darkens by the
    /// same vision.
    pub vision: TeamVision,
    /// The side's sight as the main view's fog of war shows it, which the
    /// map lights.
    pub sight: FogOfWarSight,
}

/// [`minimap_state_from`] over the arena and tanks as they are now.
#[cfg(test)]
pub(crate) fn minimap_state(scene: &Scene, player: GameObject) -> MinimapState {
    minimap_state_from(scene, player, &VisionFrame::read(scene))
}

/// Reads the map of the scene `player`'s HUD shows: the arena of `frame`,
/// facing the way `player`'s camera does, and every tank, sentry and live
/// target, with what `player`'s side sees now over `frame`'s arena.
pub(crate) fn minimap_state_from(
    scene: &Scene,
    player: GameObject,
    frame: &VisionFrame,
) -> MinimapState {
    let right = camera_right(scene, player).unwrap_or_else(|| vec2(1.0, 0.0));
    let (view, floor, cover) = match &frame.arena {
        Some(arena) => (
            MinimapView::fitting(arena.floor, right),
            arena.floor_is_edge.then_some(arena.floor),
            arena.cover.clone(),
        ),
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

    let vision = frame.vision(scene, player);
    let mut blips: Vec<MinimapBlip> = crate::vision::units(scene)
        .into_iter()
        .filter_map(|object| blip(scene, player, object))
        .collect();
    // The player's chevron is painted last, over any blip it overlaps.
    blips.sort_by_key(|blip| blip.kind == BlipKind::Player);
    MinimapState {
        player_position: ground(scene.world_transform(player).position),
        vision_range: scene
            .read_component::<ComponentTankController, _, _>(player, |controller| {
                controller.vision_range
            })
            .unwrap_or(0.0)
            .max(0.0),
        sight: side_sight(&vision),
        vision,
        view,
        floor,
        cover,
        blips,
    }
}

fn blip(scene: &Scene, player: GameObject, object: GameObject) -> Option<MinimapBlip> {
    if scene
        .read_component::<ComponentHealth, _, _>(object, |health| health.dead)
        .unwrap_or(false)
    {
        return None;
    }
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
    // Checked once the object is known to be a blip: walking the hierarchy
    // for every prop of the arena costs more than the rest of the map.
    if !scene.is_visible_in_hierarchy(object) {
        return None;
    }

    let transform = scene.world_transform(object);
    Some(MinimapBlip {
        id: scene.uuid(object),
        presence: None,
        friendly: object == player || same_side(scene, player, object),
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

fn rect_center(rect: UiRect) -> UiPoint {
    UiPoint::new(
        (rect.min.x + rect.max.x) * 0.5,
        (rect.min.y + rect.max.y) * 0.5,
    )
}

/// Clip convex geometry against a polygonal circle before sending it to the UI.
/// This avoids square map corners and contacts bleeding over its steel rim.
/// Retains only the current geometry, independent of player/camera movement.
///
/// It also keeps the sight rays last cast, with the sight they were cast
/// for: casting every ray against the cover is most of what the map costs,
/// and the sight holds still while the side's tanks do.
#[derive(Default)]
pub(super) struct BoundaryCache {
    floor: Option<GroundRect>,
    cover: Vec<GroundRect>,
    edges: Vec<(Vec2, Vec2)>,
    rays: Option<(FogOfWarSight, Vec<SightRays>)>,
}

/// How far, in metres, the floor or a block of cover moves before the map's
/// outline is traced again. Cover that creeps by a fraction of a millimetre
/// every frame, as a slowly scaled prop does, would otherwise retrace the
/// whole outline every frame for a change no map pixel shows.
const OUTLINE_TOLERANCE: f32 = 1e-3;

/// Whether `a` and `b` lie within [`OUTLINE_TOLERANCE`] of each other.
fn nearly_same(a: &GroundRect, b: &GroundRect) -> bool {
    let near = |a: Vec2, b: Vec2| (a - b).abs().max() <= OUTLINE_TOLERANCE;
    near(a.center, b.center)
        && near(a.half_axes[0], b.half_axes[0])
        && near(a.half_axes[1], b.half_axes[1])
}

impl BoundaryCache {
    fn edges(&mut self, floor: Option<GroundRect>, cover: &[GroundRect]) -> &[(Vec2, Vec2)] {
        let same_floor = match (&self.floor, &floor) {
            (Some(a), Some(b)) => nearly_same(a, b),
            (None, None) => true,
            _ => false,
        };
        let same_cover = self.cover.len() == cover.len()
            && self.cover.iter().zip(cover).all(|(a, b)| nearly_same(a, b));
        if !same_floor || !same_cover {
            self.edges = playable_boundary(floor, cover);
            self.floor = floor;
            self.cover.clear();
            self.cover.extend_from_slice(cover);
        }
        &self.edges
    }

    /// The `count` sight rays of each of `sight`'s viewers, cast again only
    /// when `sight` changed since the last call.
    fn sight_rays(&mut self, sight: &FogOfWarSight, count: usize) -> &[SightRays] {
        let current = self.rays.as_ref().is_some_and(|(cast_for, rays)| {
            cast_for == sight && rays.first().is_none_or(|ray| ray.through.len() == count)
        });
        if !current {
            self.rays = Some((sight.clone(), sight_rays(sight, count)));
        }
        self.rays.as_ref().map_or(&[], |(_, rays)| rays)
    }
}

fn corners(area: GroundRect) -> [Vec2; 4] {
    let [a, b] = area.half_axes;
    [
        area.center - a - b,
        area.center + a - b,
        area.center + a + b,
        area.center - a + b,
    ]
}

fn contains(area: GroundRect, point: Vec2) -> bool {
    crate::vision::cover_entry(point, point, area).is_some()
}

/// Boundary of floor minus solid cover. Splitting intersections and checking the
/// free-space side removes shared tile edges without assuming an axis-aligned grid.
fn playable_boundary(floor: Option<GroundRect>, cover: &[GroundRect]) -> Vec<(Vec2, Vec2)> {
    let mut result = Vec::new();
    let mut areas = cover
        .iter()
        .copied()
        .map(|a| (a, false))
        .collect::<Vec<_>>();
    if let Some(floor) = floor {
        areas.push((floor, true));
    }
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    for &(area, is_floor) in &areas {
        let vertices = corners(area);
        for i in 0..4 {
            let a = vertices[i];
            let b = vertices[(i + 1) % 4];
            let d = b - a;
            let Some(mut side) = vec2(-d.y, d.x).try_normalize(1e-6) else {
                continue;
            };
            if side.dot(&((a + b) * 0.5 - area.center)) < 0.0 {
                side = -side;
            }
            if is_floor {
                side = -side;
            }
            let mut cuts = vec![0.0, 1.0];
            for &(other, _) in &areas {
                let other = corners(other);
                for j in 0..4 {
                    let c = other[j];
                    let e = other[(j + 1) % 4] - c;
                    let denom = cross(d, e);
                    if denom.abs() > 1e-6 {
                        let t = cross(c - a, e) / denom;
                        let u = cross(c - a, d) / denom;
                        if t > 0.0 && t < 1.0 && (-1e-5..=1.00001).contains(&u) {
                            cuts.push(t);
                        }
                    } else if cross(c - a, d).abs() < 1e-4 {
                        let t = (c - a).dot(&d) / d.norm_squared();
                        if t > 0.0 && t < 1.0 {
                            cuts.push(t);
                        }
                    }
                }
            }
            cuts.sort_by(f32::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
            for span in cuts.windows(2) {
                let mid = a + d * ((span[0] + span[1]) * 0.5) + side * 0.002;
                if floor.is_none_or(|f| contains(f, mid))
                    && !cover.iter().any(|&c| contains(c, mid))
                {
                    result.push((a + d * span[0], a + d * span[1]));
                }
            }
        }
    }
    result
}

fn clip_line_circle(
    a: (f32, f32),
    b: (f32, f32),
    center: (f32, f32),
    radius: f32,
) -> Option<((f32, f32), (f32, f32))> {
    let start = vec2(a.0 - center.0, a.1 - center.1);
    let d = vec2(b.0 - a.0, b.1 - a.1);
    let len = d.norm_squared();
    if len < 1e-8 {
        return None;
    }
    let k = start.dot(&d);
    let disc = k * k - len * (start.norm_squared() - radius * radius);
    if disc < 0.0 {
        return None;
    }
    let near = ((-k - disc.sqrt()) / len).max(0.0);
    let far = ((-k + disc.sqrt()) / len).min(1.0);
    if near >= far {
        return None;
    }
    Some((
        (a.0 + d.x * near, a.1 + d.y * near),
        (a.0 + d.x * far, a.1 + d.y * far),
    ))
}

pub(super) fn paint_map(
    p: &mut Painter,
    state: &MinimapState,
    cache: &std::sync::Mutex<BoundaryCache>,
    x: f32,
    y: f32,
    radius: f32,
) {
    let vision = state.vision_range;
    p.circle(x, y, radius + 4.0, STEEL);
    p.circle(x, y, radius, UiColor::rgba(3, 8, 12, 255));
    let rect = UiRect::from_min_size(
        UiPoint::new(x - radius, y - radius),
        UiSize::new(radius * 2.0, radius * 2.0),
    );
    let extent = vision.max(1.0) * MAP_REACH;
    let view = MinimapView::facing(
        state.player_position,
        vec2(extent, extent),
        state.view.right,
    );
    let mapped = |world: Vec2| {
        let point = view.map_point(world, rect);
        (point.x, point.y)
    };
    // Keep other friendly tanks' light inside the map's rim.
    let inside = |world: Vec2| {
        let offset = world - state.player_position;
        let limit = extent * 0.97;
        if offset.norm() > limit {
            state.player_position + offset.normalize() * limit
        } else {
            world
        }
    };
    // Concentric, vertex-coloured rays light the ground every tank lending
    // its sight sees, as the main view's fog of war shows it: out along its
    // sight rays to where the fog's blockers stop them, fading with distance
    // and with the tank's weight as the main view's fog does.
    let mut points = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    const RAYS: usize = 240;
    const RINGS: usize = 12;
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (viewer, rays) in state
        .sight
        .viewers
        .iter()
        .zip(cache.sight_rays(&state.sight, RAYS))
    {
        let count = rays.through.len();
        let first = points.len() as u32;
        for i in 0..=count {
            let ray = i % count;
            let angle = ray as f32 * std::f32::consts::TAU / count as f32;
            let length = rays.through[ray].min(viewer.fade_end).max(0.0);
            let direction = vec2(angle.cos(), angle.sin());
            for ring in 0..=RINGS {
                let distance = length * ring as f32 / RINGS as f32;
                points.push(mapped(inside(viewer.position + direction * distance)));
                let light = viewer.sight(distance);
                colors.push(UiColor::rgba(28, 145, 191, (light * 110.0) as u8));
                if i > 0 && ring > 0 {
                    let b = first + (i * (RINGS + 1) + ring) as u32;
                    let a = b - (RINGS + 1) as u32;
                    indices.extend([a - 1, a, b, a - 1, b, b - 1]);
                }
            }
        }
    }
    p.gradient(&points, &colors, &indices);
    let seeing = state.vision.points();
    for &(a, b) in cache.edges(state.floor, &state.cover) {
        // Most of the arena's outline lies off the map; only what the map
        // shows asks what the side sees, a line of sight per point.
        let Some((from, to)) = clip_line_circle(mapped(a), mapped(b), (x, y), radius - 2.0) else {
            continue;
        };
        let seen = seeing.visible(a) || seeing.visible(b) || seeing.visible((a + b) * 0.5);
        p.line(
            from,
            to,
            if seen {
                UiColor::rgba(107, 161, 164, 255)
            } else {
                UiColor::rgba(40, 61, 65, 255)
            },
            1.8,
        );
    }
    for blip in &state.blips {
        // An enemy fades in and out with fog, as it does in the main view.
        let opacity = if blip.friendly {
            1.0
        } else {
            let visible =
                !state.vision.concealed.contains(&blip.id) && seeing.visible(blip.position);
            blip.presence.unwrap_or(if visible { 1.0 } else { 0.0 })
        };
        if opacity <= 0.0 {
            continue;
        }
        let point = mapped(blip.position);
        if (point.0 - x).hypot(point.1 - y) > radius - 12.0 {
            continue;
        }
        if blip.kind == BlipKind::Player {
            let dir = view.map_direction(blip.heading.unwrap_or(vec2(0.0, 1.0)));
            let side = vec2(-dir.y, dir.x);
            // Concave chevron, split into two convex blade triangles.
            for (size, color) in [(14.0, BLACK), (11.0, LIGHT)] {
                let at = |forward: f32, sideways: f32| {
                    (
                        point.0 + dir.x * forward * size + side.x * sideways * size,
                        point.1 + dir.y * forward * size + side.y * sideways * size,
                    )
                };
                for sign in [-1.0, 1.0] {
                    p.polygon(
                        &[at(1.0, 0.0), at(-0.75, sign * 0.7), at(-0.25, 0.0)],
                        color,
                    );
                }
            }
        } else {
            p.circle(point.0, point.1, 6.5, BLACK.with_alpha(opacity));
            p.circle(
                point.0,
                point.1,
                5.0,
                (if blip.friendly { CYAN } else { RED }).with_alpha(opacity),
            );
        }
    }
    for i in 0..24 {
        let a = i as f32 * std::f32::consts::TAU / 24.0;
        let major = i % 6 == 0;
        let inside = radius + if major { 0.0 } else { 2.0 };
        p.line(
            (x + a.cos() * inside, y + a.sin() * inside),
            (x + a.cos() * (radius + 4.0), y + a.sin() * (radius + 4.0)),
            MUTED.with_alpha(if major { 0.8 } else { 0.4 }),
            if major { 2.5 } else { 1.5 },
        );
    }
    let north = view.map_direction(vec2(0.0, 1.0));
    let nx = x + north.x * (radius + 2.0);
    let ny = y + north.y * (radius + 2.0);
    p.circle(nx, ny, 17.0, STEEL);
    p.circle(nx, ny, 14.5, BLACK);
    p.middle(nx, ny, "N", 24.0, LIGHT, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::ComponentArena;
    use calyx_engine::component::{ColliderShape, ComponentCollider};
    use calyx_engine::math::Transform;
    use nalgebra_glm::{vec3, Vec3};

    #[test]
    fn playable_contours_remove_shared_tile_seams_and_keep_obstacle_edges() {
        let floor = GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0));
        let cover = [
            GroundRect::axis_aligned(vec2(-1.0, 0.0), vec2(2.0, 2.0)),
            GroundRect::axis_aligned(vec2(1.0, 0.0), vec2(2.0, 2.0)),
        ];
        let edges = playable_boundary(Some(floor), &cover);
        assert!(!edges
            .iter()
            .any(|(a, b)| a.x.abs() < 1e-4 && b.x.abs() < 1e-4));
        let length: f32 = edges.iter().map(|(a, b)| (b - a).norm()).sum();
        assert!(
            (length - 92.0).abs() < 1e-3,
            "floor80 + obstacle12, got{length}"
        );
        let wall = [GroundRect::axis_aligned(vec2(0.0, -9.0), vec2(20.0, 2.0))];
        let edges = playable_boundary(Some(floor), &wall);
        assert!(!edges.iter().any(|(a, b)| a.y < -9.9 && b.y < -9.9));
        assert!(edges
            .iter()
            .any(|(a, b)| (a.y + 8.0).abs() < 1e-4 && (b.y + 8.0).abs() < 1e-4));
    }

    #[test]
    fn authored_cover_uses_current_collider_poses_instead_of_grid_cells() {
        let mut scene = calyx_engine::test_support::test_scene();
        let arena = scene.create(None, None);
        scene.set_transform(arena, &Transform::from_xyz(0.0, 0.0, 11.0).matrix());
        scene.add_component(
            arena,
            ComponentArena {
                generate_on_start: false,
                ..Default::default()
            },
        );
        let cover = scene.create(
            Some(calyx_engine::component::ComponentID {
                name: "Arena Cover Test".into(),
                ..Default::default()
            }),
            Some(arena),
        );
        scene.add_component(
            cover,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: vec3(1.2, 0.6, 0.4),
                },
                ..Default::default()
            },
        );
        scene.set_transform(
            cover,
            &Transform::from_components(
                vec3(4.0, 0.6, 1.0),
                nalgebra::UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        let player = scene.create(None, None);
        let state = minimap_state(&scene, player);
        assert_eq!(state.cover.len(), 1);
        assert_eq!(state.cover[0].center, vec2(4.0, 12.0));
        assert!((state.cover[0].half_axes[0].norm() - 1.2).abs() < 1e-4);
        assert!((state.cover[0].half_axes[1].norm() - 0.4).abs() < 1e-4);
        assert!(state.cover[0].half_axes[0].x.abs() < 1e-4);
        scene.set_transform(cover, &Transform::from_xyz(8.0, 0.6, 1.0).matrix());
        assert_eq!(
            minimap_state(&scene, player).cover[0].center,
            vec2(8.0, 12.0)
        );
    }

    #[test]
    fn interior_walls_are_outlined_and_block_sight_while_the_perimeter_bounds_the_floor() {
        let mut scene = calyx_engine::test_support::test_scene();
        let arena = scene.create(None, None);
        scene.add_component(
            arena,
            ComponentArena {
                generate_on_start: false,
                ..Default::default()
            },
        );
        let named = |scene: &mut Scene, name: &str, parent| {
            scene.create(
                Some(calyx_engine::component::ComponentID {
                    name: name.into(),
                    ..Default::default()
                }),
                Some(parent),
            )
        };
        let solid = |scene: &mut Scene, object, position: Vec3, half_extents| {
            scene.set_transform(
                object,
                &Transform::from_xyz(position.x, 1.0, position.z).matrix(),
            );
            scene.add_component(
                object,
                ComponentCollider {
                    shape: ColliderShape::Cuboid { half_extents },
                    ..Default::default()
                },
            );
        };
        // A perimeter strip, as the arena's own child, and an interior wall
        // run under its wall.
        let perimeter = named(&mut scene, "Arena Wall North collision", arena);
        solid(
            &mut scene,
            perimeter,
            vec3(0.0, 0.0, 9.0),
            vec3(10.0, 1.0, 0.6),
        );
        let wall = named(&mut scene, "Arena Wall Middle", arena);
        let run = named(&mut scene, "Arena Wall Middle collision 0", wall);
        solid(&mut scene, run, vec3(2.0, 0.0, 0.0), vec3(0.6, 1.0, 3.0));
        let player = scene.create(None, None);

        let geometry = arena_geometry(&scene).unwrap();
        assert_eq!(geometry.sight_blockers.len(), 2, "both walls block sight");
        let state = minimap_state(&scene, player);
        assert_eq!(state.cover.len(), 1, "only the interior wall is cover");
        assert_eq!(state.cover[0].center, vec2(2.0, 0.0));
        // The map outlines the interior wall's four sides.
        let edges = playable_boundary(state.floor, &state.cover);
        assert!(edges
            .iter()
            .any(|(a, b)| (a.x - 1.4).abs() < 1e-4 && (b.x - 1.4).abs() < 1e-4));
        assert!(edges
            .iter()
            .any(|(a, b)| (a.x - 2.6).abs() < 1e-4 && (b.x - 2.6).abs() < 1e-4));
    }

    #[test]
    fn transformed_cover_and_floor_share_world_geometry() {
        let mut transform = Transform::from_xyz(10.0, 0.0, 3.0);
        transform.rotation =
            nalgebra::UnitQuaternion::from_euler_angles(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        let cover = GroundRect::placed(&transform, vec3(0.0, 0.0, 0.0), vec2(4.0, 2.0));
        assert!((cover_entry(vec2(5.0, 3.0), vec2(15.0, 3.0), cover).unwrap() - 0.4).abs() < 1e-4);
        let view = MinimapView::fitting(cover, vec2(1.0, 0.0));
        let rect = UiRect::from_min_size(UiPoint::ZERO, UiSize::new(100.0, 100.0));
        assert_eq!(view.map_point(cover.center, rect), UiPoint::new(50.0, 50.0));
    }

    #[test]
    fn contour_clipping_handles_crossings_outside_and_tangents() {
        assert_eq!(
            clip_line_circle((-20.0, 0.0), (20.0, 0.0), (0.0, 0.0), 10.0),
            Some(((-10.0, 0.0), (10.0, 0.0)))
        );
        assert!(clip_line_circle((-20.0, 11.0), (20.0, 11.0), (0.0, 0.0), 10.0).is_none());
        assert!(clip_line_circle((-20.0, 10.0), (20.0, 10.0), (0.0, 0.0), 10.0).is_none());
        assert_eq!(
            clip_line_circle((0.0, 0.0), (5.0, 0.0), (0.0, 0.0), 10.0),
            Some(((0.0, 0.0), (5.0, 0.0)))
        );
    }

    #[test]
    fn contour_cache_reuses_geometry_and_invalidates_moved_cover() {
        let floor = Some(GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0)));
        let mut cover = [GroundRect::axis_aligned(vec2(2.0, 0.0), vec2(2.0, 2.0))];
        let mut cache = BoundaryCache::default();
        let original = cache.edges(floor, &cover).to_vec();
        let allocation = cache.edges.as_ptr();
        assert_eq!(cache.edges(floor, &cover), original);
        assert_eq!(cache.edges.as_ptr(), allocation);
        cover[0].center.x = 4.0;
        assert_ne!(cache.edges(floor, &cover), original);
        assert_eq!(cache.edges(None, &[]).len(), 0);
    }

    #[test]
    fn contour_cache_keeps_the_outline_while_cover_creeps_within_the_tolerance() {
        let floor = Some(GroundRect::axis_aligned(vec2(0.0, 0.0), vec2(20.0, 20.0)));
        let mut cover = [GroundRect::axis_aligned(vec2(2.0, 0.0), vec2(2.0, 2.0))];
        let mut cache = BoundaryCache::default();
        let original = cache.edges(floor, &cover).to_vec();
        let allocation = cache.edges.as_ptr();
        // A prop scaled a hair every frame keeps the traced outline...
        for _ in 0..10 {
            cover[0].half_axes[0].x -= OUTLINE_TOLERANCE / 20.0;
            assert_eq!(cache.edges(floor, &cover), original);
            assert_eq!(cache.edges.as_ptr(), allocation);
        }
        // ...until it has moved past the tolerance since it was traced.
        cover[0].half_axes[0].x -= OUTLINE_TOLERANCE;
        assert_eq!(cache.edges(floor, &cover), playable_boundary(floor, &cover));
        assert_ne!(cache.edges(floor, &cover), original);
    }

    #[test]
    fn live_cover_moves_and_visibility_changes_follow_generated_children() {
        let mut scene = calyx_engine::test_support::test_scene();
        let player = scene.create(None, None);
        scene.add_component(player, ComponentTankController::default());
        let arena = scene.create(None, None);
        scene.add_component(arena, ComponentArena::default());
        let cover = scene.create(None, Some(arena));
        scene.add_component(cover, ComponentCollider::default());
        scene.set_transform(
            cover,
            &Transform::from_components(
                vec3(3.0, 1.0, 0.0),
                Default::default(),
                vec3(2.0, 2.0, 2.0),
            )
            .matrix(),
        );
        assert!(!minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));
        scene.set_transform(
            cover,
            &Transform::from_components(
                vec3(3.0, 1.0, 5.0),
                Default::default(),
                vec3(2.0, 2.0, 2.0),
            )
            .matrix(),
        );
        assert!(minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));
    }

    #[test]
    fn a_tank_blocker_leaves_the_minimap_seeing_across_it() {
        let mut scene = calyx_engine::test_support::test_scene();
        let player = scene.create(None, None);
        scene.add_component(player, ComponentTankController::default());
        let arena = scene.create(None, None);
        scene.add_component(arena, ComponentArena::default());
        let cover = scene.create(None, Some(arena));
        scene.add_component(cover, ComponentCollider::default());
        scene.set_transform(
            cover,
            &Transform::from_components(
                vec3(3.0, 1.0, 0.0),
                Default::default(),
                vec3(2.0, 2.0, 2.0),
            )
            .matrix(),
        );
        assert!(!minimap_state(&scene, player).vision.visible(vec2(6.0, 0.0)));

        let _ = scene.write_component::<ComponentCollider, _>(cover, |collider| {
            collider.groups = crate::collision::TANK_BLOCKER;
        });
        let state = minimap_state(&scene, player);
        assert!(state.vision.visible(vec2(6.0, 0.0)));
        assert_eq!(state.cover.len(), 1, "the map still outlines the blocker");
    }

    #[test]
    fn the_map_marks_only_units_and_paints_the_player_last() {
        let mut scene = calyx_engine::test_support::test_scene();
        let player = scene.create(None, None);
        scene.add_component(player, ComponentTankController::default());
        // Props that are not units get no blip, however many there are.
        for _ in 0..20 {
            let prop = scene.create(None, None);
            scene.add_component(prop, ComponentCollider::default());
        }
        let enemy = scene.create(None, None);
        scene.add_component(enemy, ComponentTankController::default());
        let target = scene.create(None, None);
        scene.add_component(target, ComponentProjectileTarget::default());

        let blips = minimap_state(&scene, player).blips;
        let mut ids = blips.iter().map(|blip| blip.id).collect::<Vec<_>>();
        assert_eq!(ids.len(), 3);
        assert_eq!(blips.last().map(|blip| blip.kind), Some(BlipKind::Player));
        ids.sort();
        let mut expected = vec![scene.uuid(player), scene.uuid(enemy), scene.uuid(target)];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn sight_rays_are_cast_again_only_when_the_sight_changes() {
        use calyx_engine::component::{FogOfWarBlocker, FogOfWarViewer};
        let viewer = FogOfWarViewer {
            position: vec2(0.0, 0.0),
            fade_start: 10.0,
            fade_end: 12.0,
            fade_curve: 0.0,
            weight: 1.0,
        };
        let mut sight = FogOfWarSight {
            viewers: vec![viewer],
            blockers: vec![FogOfWarBlocker {
                center: vec2(4.0, 0.0),
                half_axes: [vec2(1.0, 0.0), vec2(0.0, 1.0)],
            }],
            border: Default::default(),
        };
        let mut cache = BoundaryCache::default();
        let first = cache.sight_rays(&sight, 64).to_vec();
        assert_eq!(first, sight_rays(&sight, 64));
        let kept = cache.sight_rays(&sight, 64).as_ptr();
        assert_eq!(
            cache.sight_rays(&sight, 64).as_ptr(),
            kept,
            "an unchanged sight is not cast again"
        );

        sight.viewers[0].position = vec2(0.0, 3.0);
        let moved = cache.sight_rays(&sight, 64).to_vec();
        assert_eq!(moved, sight_rays(&sight, 64));
        assert_ne!(moved, first);
        assert_eq!(cache.sight_rays(&sight, 32).len(), 1);
        assert_eq!(cache.sight_rays(&sight, 32)[0].through.len(), 32);
    }
}
