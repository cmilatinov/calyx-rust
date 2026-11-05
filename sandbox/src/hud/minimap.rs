//! A top-down map of the arena, drawn from world positions each frame.

use super::paint::{Painter, BLACK, CYAN, LIGHT, MUTED, RED, STEEL};
use crate::arena::ComponentArena;
use crate::combat::combat_team;
use crate::sentry::ComponentSentry;
use crate::spawn::ComponentRespawnState;
use crate::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
use calyx_engine::component::ComponentCollider;
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Scene};
use calyx_engine::ui::{UiColor, UiPoint, UiRect, UiSize};
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
    pub friendly: bool,
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
    pub player_position: Vec2,
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
            .map(|arena| (object, scene.world_transform(object), arena))
    });

    let (view, floor, cover) = match arena {
        Some((arena_object, transform, ((width, depth), cell_size, cover))) => {
            let floor = GroundRect::placed(&transform, Vec3::zeros(), vec2(width, depth));
            let authored_cover = cover
                .iter()
                .map(|position| {
                    GroundRect::placed(&transform, *position, vec2(cell_size, cell_size))
                })
                .collect();
            let children = scene.children(arena_object).collect::<Vec<_>>();
            let cover = if children.is_empty() {
                authored_cover
            } else {
                children
                    .into_iter()
                    .filter(|object| {
                        scene.is_visible_in_hierarchy(*object)
                            && scene.is_component_enabled::<ComponentCollider>(*object)
                            && scene.transform(*object).position.y > 0.0
                    })
                    .map(|object| {
                        GroundRect::placed(
                            &scene.world_transform(object),
                            Vec3::zeros(),
                            vec2(1.0, 1.0),
                        )
                    })
                    .collect()
            };
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
        player_position: ground(scene.world_transform(player).position),
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
    if !scene.is_visible_in_hierarchy(object)
        || scene
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

    let transform = scene.world_transform(object);
    Some(MinimapBlip {
        friendly: object == player
            || combat_team(scene, player)
                .zip(combat_team(scene, object))
                .is_some_and(|(a, b)| a == b),
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

/// Segment entry in a transformed ground footprint. Inverting its two half axes
/// handles rotated, scaled and sheared arena parents without axis-aligned leaks.
fn cover_entry(start: Vec2, end: Vec2, cover: GroundRect) -> Option<f32> {
    let [a, b] = cover.half_axes;
    let det = a.x * b.y - a.y * b.x;
    if det.abs() < 1e-6 {
        return None;
    }
    let local = |v: Vec2| vec2((v.x * b.y - v.y * b.x) / det, (a.x * v.y - a.y * v.x) / det);
    let origin = local(start - cover.center);
    let direction = local(end - start);
    let mut near = 0.0f32;
    let mut far = 1.0f32;
    for axis in 0..2 {
        if direction[axis].abs() < 1e-6 {
            if origin[axis].abs() > 1.0 {
                return None;
            }
        } else {
            let a = (-1.0 - origin[axis]) / direction[axis];
            let b = (1.0 - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    (far >= 0.0 && near <= 1.0).then_some(near.max(0.0))
}
impl MinimapState {
    pub fn visible(&self, position: Vec2, range: f32) -> bool {
        (position - self.player_position).norm() <= range.max(0.0)
            && !self.cover.iter().any(|cover| {
                cover_entry(self.player_position, position, *cover).is_some_and(|t| t < 0.999)
            })
    }
    fn sight_end(&self, angle: f32, range: f32) -> Vec2 {
        let end = self.player_position + vec2(angle.cos(), angle.sin()) * range.max(0.0);
        let t = self
            .cover
            .iter()
            .filter_map(|cover| cover_entry(self.player_position, end, *cover))
            .fold(1.0f32, f32::min);
        self.player_position + (end - self.player_position) * t
    }
}

/// Clip convex geometry against a polygonal circle before sending it to the UI.
/// This avoids square map corners and contacts bleeding over its steel rim.
/// Retains only the current geometry, independent of player/camera movement.
#[derive(Default)]
pub(super) struct BoundaryCache {
    floor: Option<GroundRect>,
    cover: Vec<GroundRect>,
    edges: Vec<(Vec2, Vec2)>,
}
impl BoundaryCache {
    fn edges(&mut self, floor: Option<GroundRect>, cover: &[GroundRect]) -> &[(Vec2, Vec2)] {
        if self.floor != floor || self.cover != cover {
            self.edges = playable_boundary(floor, cover);
            self.floor = floor;
            self.cover.clear();
            self.cover.extend_from_slice(cover);
        }
        &self.edges
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
    cover_entry(point, point, area).is_some()
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
    vision: f32,
) {
    p.circle(x, y, radius + 4.0, STEEL);
    p.circle(x, y, radius, UiColor::rgba(3, 8, 12, 255));
    let rect = UiRect::from_min_size(
        UiPoint::new(x - radius, y - radius),
        UiSize::new(radius * 2.0, radius * 2.0),
    );
    let extent = vision.max(1.0) / 0.70;
    let view = MinimapView::facing(
        state.player_position,
        vec2(extent, extent),
        state.view.right,
    );
    let mapped = |world: Vec2| {
        let point = view.map_point(world, rect);
        (point.x, point.y)
    };
    // Concentric, vertex-coloured rays produce a soft radial falloff, with
    // every ray terminated by actual live cover. Nothing lights behind a wall.
    let mut points = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    const RAYS: usize = 192;
    const RINGS: usize = 12;
    for i in 0..=RAYS {
        let angle = i as f32 * std::f32::consts::TAU / RAYS as f32;
        let end = state.sight_end(angle, vision);
        let length = (end - state.player_position).norm();
        for ring in 0..=RINGS {
            let t = ring as f32 / RINGS as f32;
            points.push(mapped(
                state.player_position + (end - state.player_position) * t,
            ));
            let distance = t * length / vision.max(1.0);
            let radial = (1.0 - distance.powi(2)).clamp(0.0, 1.0);
            let edge = if ring == RINGS { 0.0 } else { 1.0 };
            colors.push(UiColor::rgba(28, 145, 191, (radial * edge * 110.0) as u8));
            if i > 0 && ring > 0 {
                let b = (i * (RINGS + 1) + ring) as u32;
                let a = b - (RINGS + 1) as u32;
                indices.extend([a - 1, a, b, a - 1, b, b - 1]);
            }
        }
    }
    p.gradient(&points, &colors, &indices);
    let mut boundary = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for &(a, b) in boundary.edges(state.floor, &state.cover) {
        let seen = state.visible(a, vision)
            || state.visible(b, vision)
            || state.visible((a + b) * 0.5, vision);
        if let Some((a, b)) = clip_line_circle(mapped(a), mapped(b), (x, y), radius - 2.0) {
            p.line(
                a,
                b,
                if seen {
                    UiColor::rgba(107, 161, 164, 255)
                } else {
                    UiColor::rgba(40, 61, 65, 255)
                },
                1.8,
            );
        }
    }
    for blip in &state.blips {
        if !blip.friendly && !state.visible(blip.position, vision) {
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
            p.circle(point.0, point.1, 6.5, BLACK);
            p.circle(
                point.0,
                point.1,
                5.0,
                if blip.friendly { CYAN } else { RED },
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
    fn state(cover: Vec<GroundRect>) -> MinimapState {
        MinimapState {
            view: MinimapView::facing(Vec2::zeros(), vec2(20.0, 20.0), vec2(1.0, 0.0)),
            floor: None,
            cover,
            blips: Vec::new(),
            player_position: Vec2::zeros(),
        }
    }
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
    fn vision_hides_behind_cover_and_beyond_range() {
        let map = state(vec![GroundRect::axis_aligned(
            vec2(4.0, 0.0),
            vec2(2.0, 4.0),
        )]);
        assert!(map.visible(vec2(2.0, 0.0), 10.0));
        assert!(!map.visible(vec2(6.0, 0.0), 10.0));
        assert!(map.visible(vec2(6.0, 6.0), 10.0));
        assert!(!map.visible(vec2(0.0, 11.0), 10.0));
        assert_eq!(map.sight_end(0.0, 10.0), vec2(3.0, 0.0));
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
        assert!(!minimap_state(&scene, player).visible(vec2(6.0, 0.0), 10.0));
        scene.set_transform(
            cover,
            &Transform::from_components(
                vec3(3.0, 1.0, 5.0),
                Default::default(),
                vec3(2.0, 2.0, 2.0),
            )
            .matrix(),
        );
        assert!(minimap_state(&scene, player).visible(vec2(6.0, 0.0), 10.0));
    }
}
