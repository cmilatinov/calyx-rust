use crate::math::catmull_rom;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3, Vec4};

/// Most rows (along the river) a [`WaterPath`] meshes.
pub const MAX_WATER_ROWS: usize = 8192;
/// Most columns (across the river) a [`WaterPath`] meshes.
pub const MAX_WATER_COLUMNS: usize = 256;
/// Smallest cell a [`WaterPath`] meshes, in local units.
pub const MIN_WATER_CELL: f32 = 0.05;

/// How far, as a share of a cell, the mesh moves each inner vertex off its
/// regular position, so the facets come out irregular rather than ruled.
const VERTEX_JITTER: f32 = 0.3;
/// Segments each stretch of the centreline is flattened into before it is
/// resampled at the cell size.
const CURVE_STEPS: usize = 48;
/// Obstacle distances are clamped to this, in local units: foam rings never
/// reach further.
const MAX_OBSTACLE_DISTANCE: f32 = 50.0;

/// The farthest a vertex lies from the shore when its own shore opens into
/// other water and no shore meeting land is nearer, in local units.
const MAX_OPEN_SHORE_DISTANCE: f32 = 50.0;

/// One control point of a river's centreline, in its owner's local XZ plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterPathPoint {
    /// Local X and Z of the centreline.
    pub position: Vec2,
    /// Width of the water from shore to shore here.
    pub width: f32,
}

/// Something standing in the water, which foam rings: a circle in the
/// owner's local XZ plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterCircle {
    pub center: Vec2,
    pub radius: f32,
}

/// The faceted surface of a river: a band along a smooth centreline through
/// `points`, as wide as their widths, on local Y = 0.
///
/// The centreline is a centripetal Catmull-Rom curve through the points, so
/// it passes through each without loops or cusps; the width eases between
/// the points' widths the same way. The band is cut into rows about
/// `cell_size` apart along the curve and as many columns across as fit the
/// widest point, each cell two irregular triangles: inner vertices are moved
/// off the regular grid by a fixed pseudo-random amount, and the cells
/// alternate their diagonal. Edge vertices stay on the shore.
///
/// Each vertex knows where it lies in river coordinates: `s`, the distance
/// along the centreline from the first point (the direction the water
/// flows), and `t`, the signed distance across it, positive on the left.
/// The shore is the band's two long edges; its ends are open water.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterPath {
    pub points: Vec<WaterPathPoint>,
    /// About how long and wide each cell is, in local units.
    pub cell_size: f32,
    /// What stands in the water.
    pub obstacles: Vec<WaterCircle>,
    /// Where the river opens into other water, such as a branch that joins
    /// it: no bank is cut along a shore inside these circles, and the shore
    /// there is open water, not land: the water's distance to the shore is
    /// measured to the nearest stretch of shore outside every opening, so
    /// no shallows or foam band run across the mouth and two rivers meet
    /// without a wall or a foam line between them.
    pub openings: Vec<WaterCircle>,
}

/// One cross-section of a [`WaterPath`]: where the centreline is, which way
/// it runs and how wide the water is there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterSection {
    /// Distance along the centreline from its start.
    pub s: f32,
    pub center: Vec2,
    /// Unit direction of flow.
    pub tangent: Vec2,
    /// Unit direction across, to the left of the flow.
    pub normal: Vec2,
    /// Half the width: the distance from the centreline to either shore.
    pub half_width: f32,
}

/// The triangle mesh of a [`WaterPath`], three vertices of its own per
/// triangle, so each facet carries what the water shader needs to move it
/// and shade it.
///
/// Per vertex, in the mesh channels the water shader reads them from:
/// - `positions`: local rest position, on Y = 0;
/// - `river`: its own river coordinates (`s`, `t`) (UV0);
/// - `next` and `previous`: the local XZ rest positions of the triangle's
///   next and previous corners, in drawn order (UV1 and UV2);
/// - `next_river`: the next corner's river coordinates (UV3);
/// - `extra`: the previous corner's river coordinates, then the vertex's
///   own distance to the shore and to the nearest obstacle (the tangent);
/// - `facets`: the facet's pseudo-random value in `[0, 1)`, its centroid's
///   distance to the shore, and 0 (the normal).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WaterGeometry {
    pub positions: Vec<Vec3>,
    pub river: Vec<Vec2>,
    pub next: Vec<Vec2>,
    pub previous: Vec<Vec2>,
    pub next_river: Vec<Vec2>,
    pub extra: Vec<Vec4>,
    pub facets: Vec<Vec3>,
    /// Triangle list indices, `0..positions.len()` in order.
    pub indices: Vec<u32>,
}

/// The cross-section of the banks a [`WaterPath`] cuts, in local units
/// relative to the water's rest surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BankProfile {
    /// Height of the bank top above the rest surface.
    pub height: f32,
    /// Depth of the wall's foot below the rest surface.
    pub depth: f32,
    /// Horizontal run of the wall from its foot to the top.
    pub run: f32,
    /// Width of the flat lip along the top of the bank.
    pub lip: f32,
}

/// A flat-shaded triangle mesh: vertex positions and normals and triangle
/// list indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlatGeometry {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
}

impl WaterPath {
    /// Whether the path meshes a surface: two or more finite points with a
    /// positive width, and a cell size of at least [`MIN_WATER_CELL`].
    pub fn is_valid(&self) -> bool {
        self.points.len() >= 2
            && self.cell_size.is_finite()
            && self.cell_size >= MIN_WATER_CELL
            && self.points.iter().all(|point| {
                point.position.x.is_finite()
                    && point.position.y.is_finite()
                    && point.width.is_finite()
                    && point.width > 0.0
            })
            && self
                .points
                .windows(2)
                .all(|pair| (pair[1].position - pair[0].position).norm() > 1e-4)
    }

    /// The centreline flattened into short straight pieces: each point's
    /// local XZ and width, from the first control point to the last.
    fn flattened(&self) -> Vec<(Vec2, f32)> {
        let n = self.points.len();
        let point = |i: isize| -> (Vec3, f32) {
            // The ends are extended by reflecting their neighbour, so the curve
            // starts and ends at the end points heading straight at them.
            let at = |i: usize| {
                let p = self.points[i];
                (vec3(p.position.x, 0.0, p.position.y), p.width)
            };
            if i < 0 {
                let (a, wa) = at(0);
                let (b, wb) = at(1);
                (a * 2.0 - b, wa * 2.0 - wb)
            } else if i as usize >= n {
                let (a, wa) = at(n - 1);
                let (b, wb) = at(n - 2);
                (a * 2.0 - b, wa * 2.0 - wb)
            } else {
                at(i as usize)
            }
        };
        let mut out = Vec::with_capacity((n - 1) * CURVE_STEPS + 1);
        for i in 0..n as isize - 1 {
            let (p0, w0) = point(i - 1);
            let (p1, w1) = point(i);
            let (p2, w2) = point(i + 1);
            let (p3, w3) = point(i + 2);
            for step in 0..CURVE_STEPS {
                let u = step as f32 / CURVE_STEPS as f32;
                let p = centripetal(p0, p1, p2, p3, u);
                let w = catmull_rom(
                    Vec3::repeat(w0),
                    Vec3::repeat(w1),
                    Vec3::repeat(w2),
                    Vec3::repeat(w3),
                    u,
                )
                .x
                .max(w1.min(w2) * 0.25);
                out.push((vec2(p.x, p.z), w));
            }
        }
        let last = self.points[n - 1];
        out.push((last.position, last.width));
        out
    }

    /// The cross-sections at each row of the mesh, about `cell_size` apart
    /// along the centreline, from its start to its end. `None` when the path
    /// is not valid or would need more rows than [`MAX_WATER_ROWS`].
    pub fn sections(&self) -> Option<Vec<WaterSection>> {
        if !self.is_valid() {
            return None;
        }
        let line = self.flattened();
        let mut lengths = Vec::with_capacity(line.len());
        let mut length = 0.0;
        lengths.push(0.0);
        for pair in line.windows(2) {
            length += (pair[1].0 - pair[0].0).norm();
            lengths.push(length);
        }
        let rows = (length / self.cell_size).round().max(1.0) as usize;
        if rows + 1 > MAX_WATER_ROWS {
            return None;
        }
        let at = |s: f32| {
            let i = lengths
                .partition_point(|&l| l <= s)
                .clamp(1, line.len() - 1);
            let (l0, l1) = (lengths[i - 1], lengths[i]);
            let f = if l1 > l0 { (s - l0) / (l1 - l0) } else { 0.0 };
            let (p0, w0) = line[i - 1];
            let (p1, w1) = line[i];
            (p0 + (p1 - p0) * f, w0 + (w1 - w0) * f)
        };
        let step = length / rows as f32;
        let positions: Vec<_> = (0..=rows).map(|row| at(row as f32 * step)).collect();
        Some(
            (0..=rows)
                .map(|row| {
                    let previous = positions[row.saturating_sub(1)].0;
                    let next = positions[(row + 1).min(rows)].0;
                    let tangent = (next - previous).normalize();
                    let (center, width) = positions[row];
                    WaterSection {
                        s: row as f32 * step,
                        center,
                        tangent,
                        normal: vec2(-tangent.y, tangent.x),
                        half_width: width * 0.5,
                    }
                })
                .collect(),
        )
    }

    /// Columns across the river: as many cells as fit the widest point.
    fn columns(&self, sections: &[WaterSection]) -> usize {
        let widest = sections
            .iter()
            .map(|section| section.half_width * 2.0)
            .fold(0.0, f32::max);
        ((widest / self.cell_size).round() as usize).clamp(1, MAX_WATER_COLUMNS)
    }

    /// The distance from local `point` to the nearest obstacle's edge, 0
    /// inside one, at most [`MAX_OBSTACLE_DISTANCE`].
    pub fn obstacle_distance(&self, point: Vec2) -> f32 {
        self.obstacles
            .iter()
            .map(|circle| (point - circle.center).norm() - circle.radius)
            .fold(MAX_OBSTACLE_DISTANCE, f32::min)
            .max(0.0)
    }

    /// How far inside the nearest opening local `point` lies, 0 outside
    /// every opening.
    pub fn opening_depth(&self, point: Vec2) -> f32 {
        self.openings
            .iter()
            .map(|circle| circle.radius - (point - circle.center).norm())
            .fold(0.0, f32::max)
    }

    /// The river's triangle mesh, or `None` when the path is not valid.
    ///
    /// Each triangle runs counter-clockwise seen from above, as a
    /// heightfield's do, so the upper side faces outward.
    pub fn geometry(&self) -> Option<WaterGeometry> {
        let sections = self.sections()?;
        let columns = self.columns(&sections);
        let rows = sections.len() - 1;
        // Where the shore meets land: the shore at each section, on both
        // sides, outside every opening.
        let closed: Vec<Vec2> = if self.openings.is_empty() {
            Vec::new()
        } else {
            sections
                .iter()
                .flat_map(|section| {
                    [1.0, -1.0]
                        .map(|side| section.center + section.normal * section.half_width * side)
                })
                .filter(|&shore| self.opening_depth(shore) <= 0.0)
                .collect()
        };
        // A grid vertex: local XZ, river coordinates and distance to shore.
        let vertex = |column: usize, row: usize| -> (Vec2, Vec2, f32) {
            let section = sections[row];
            let across = column as f32 / columns as f32 * 2.0 - 1.0;
            let mut t = across * section.half_width;
            let mut along = 0.0;
            if column != 0 && column != columns {
                t += (hash01(column as u32, row as u32, 0x85eb) - 0.5)
                    * 2.0
                    * VERTEX_JITTER
                    * section.half_width
                    * 2.0
                    / columns as f32;
            }
            if row != 0 && row != rows {
                along = (hash01(column as u32, row as u32, 0x9e37) - 0.5)
                    * 2.0
                    * VERTEX_JITTER
                    * self.cell_size;
            }
            let position = section.center + section.tangent * along + section.normal * t;
            // Where the nearest shore opens into other water, the shore
            // that counts is the nearest stretch that meets land.
            let side = if t < 0.0 { -1.0 } else { 1.0 };
            let nearest = section.center + section.normal * section.half_width * side;
            let shore = if self.opening_depth(nearest) > 0.0 {
                closed
                    .iter()
                    .map(|&land| (land - position).norm())
                    .fold(MAX_OPEN_SHORE_DISTANCE, f32::min)
            } else {
                (section.half_width - t.abs()).max(0.0)
            };
            (position, vec2(section.s + along, t), shore)
        };
        let count = columns * rows * 6;
        let mut geometry = WaterGeometry {
            positions: Vec::with_capacity(count),
            river: Vec::with_capacity(count),
            next: Vec::with_capacity(count),
            previous: Vec::with_capacity(count),
            next_river: Vec::with_capacity(count),
            extra: Vec::with_capacity(count),
            facets: Vec::with_capacity(count),
            indices: Vec::with_capacity(count),
        };
        let mut index = 0u32;
        for row in 0..rows {
            for column in 0..columns {
                // a at (column, row), b one column to the left, c one row
                // downstream, d both.
                let a = vertex(column, row);
                let b = vertex(column + 1, row);
                let c = vertex(column, row + 1);
                let d = vertex(column + 1, row + 1);
                let split = if (column + row) % 2 == 0 {
                    [[a, b, c], [b, d, c]]
                } else {
                    [[a, d, c], [a, b, d]]
                };
                for corners in split {
                    let corners = upward(corners);
                    let centroid_shore = (corners[0].2 + corners[1].2 + corners[2].2) / 3.0;
                    let random = hash01(index, 0, 0x2c1b);
                    for i in 0..3 {
                        let (position, river, shore) = corners[i];
                        let next = corners[(i + 1) % 3];
                        let previous = corners[(i + 2) % 3];
                        geometry.indices.push(geometry.positions.len() as u32);
                        geometry.positions.push(vec3(position.x, 0.0, position.y));
                        geometry.river.push(river);
                        geometry.next.push(next.0);
                        geometry.previous.push(previous.0);
                        geometry.next_river.push(next.1);
                        geometry.extra.push(Vec4::new(
                            previous.1.x,
                            previous.1.y,
                            shore,
                            self.obstacle_distance(position),
                        ));
                        geometry.facets.push(vec3(random, centroid_shore, 0.0));
                    }
                    index += 1;
                }
            }
        }
        Some(geometry)
    }

    /// The flat-shaded banks along both shores under `profile`: from each
    /// wall's foot below the water, up and away from the river to the bank
    /// top, then along a flat lip. The wall crosses the rest surface at the
    /// shore. `None` when the path is not valid or the profile has no wall.
    pub fn banks(&self, profile: &BankProfile) -> Option<FlatGeometry> {
        let finite = [profile.height, profile.depth, profile.run, profile.lip]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0);
        if !finite || profile.height + profile.depth <= 0.0 {
            return None;
        }
        let sections = self.sections()?;
        // The wall crosses the rest surface at the shore.
        let foot = profile.run * profile.depth / (profile.depth + profile.height);
        // Distance beyond the shore and height above the rest surface of
        // each point of the cross-section, from the river outward.
        let profile_points = [
            (-foot, -profile.depth),
            (profile.run - foot, profile.height),
            (profile.run - foot + profile.lip, profile.height),
        ];
        let mut geometry = FlatGeometry::default();
        for side in [1.0f32, -1.0] {
            for pair in sections.windows(2) {
                let point = |section: &WaterSection, (out, y): (f32, f32)| {
                    let at = section.center + section.normal * side * (section.half_width + out);
                    vec3(at.x, y, at.y)
                };
                // No bank where the shore opens into other water.
                let shore = (point(&pair[0], (0.0, 0.0)) + point(&pair[1], (0.0, 0.0))).xz() * 0.5;
                if self.opening_depth(shore) > 0.0 {
                    continue;
                }
                for strip in profile_points.windows(2) {
                    let quad = [
                        point(&pair[0], strip[0]),
                        point(&pair[1], strip[0]),
                        point(&pair[1], strip[1]),
                        point(&pair[0], strip[1]),
                    ];
                    let toward = -pair[0].normal * side;
                    push_quad(&mut geometry, quad, vec3(toward.x, 1.0, toward.y));
                }
            }
        }
        Some(geometry)
    }
}

/// Adds the flat-shaded quad `corners` (two triangles), turned to face
/// `toward`: up and towards the river.
fn push_quad(geometry: &mut FlatGeometry, corners: [Vec3; 4], toward: Vec3) {
    for triangle in [[0, 1, 2], [0, 2, 3]] {
        let [a, mut b, mut c] = triangle.map(|i| corners[i]);
        // A triangle faces along (c - a) x (b - a), as the terrain's do.
        let mut facing = (c - a).cross(&(b - a));
        if facing.norm_squared() < 1e-12 {
            continue;
        }
        if facing.dot(&toward) < 0.0 {
            std::mem::swap(&mut b, &mut c);
            facing = -facing;
        }
        let facing = facing.normalize();
        for corner in [a, b, c] {
            geometry.indices.push(geometry.positions.len() as u32);
            geometry.positions.push(corner);
            geometry.normals.push(facing);
        }
    }
}

/// `corners` reordered, if needed, to run counter-clockwise seen from above,
/// as a heightfield's triangles do.
fn upward<T: Copy>(corners: [(Vec2, Vec2, T); 3]) -> [(Vec2, Vec2, T); 3] {
    let (a, b, c) = (corners[0].0, corners[1].0, corners[2].0);
    let (u, v) = (b - a, c - a);
    if u.x * v.y - u.y * v.x > 0.0 {
        corners
    } else {
        [corners[0], corners[2], corners[1]]
    }
}

/// The point a fraction `u` along the centripetal Catmull-Rom segment from
/// `p1` to `p2`, whose neighbours are `p0` and `p3`: it never loops or forms
/// a cusp, however unevenly the points are spaced.
fn centripetal(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let knot = |a: Vec3, b: Vec3| (b - a).norm().sqrt().max(1e-4);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * u;
    let lerp = |a: Vec3, b: Vec3, ta: f32, tb: f32| {
        a * ((tb - t) / (tb - ta)) + b * ((t - ta) / (tb - ta))
    };
    let a1 = lerp(p0, p1, t0, t1);
    let a2 = lerp(p1, p2, t1, t2);
    let a3 = lerp(p2, p3, t2, t3);
    let b1 = lerp(a1, a2, t0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    lerp(b1, b2, t1, t2)
}

/// A pseudo-random value in `[0, 1)` for the integers `a`, `b` and `salt`,
/// the same on every run.
fn hash01(a: u32, b: u32, salt: u32) -> f32 {
    let mut h = a
        .wrapping_mul(0x8da6_b343)
        .wrapping_add(b.wrapping_mul(0xd816_3841))
        .wrapping_add(salt.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Heightfield, HeightfieldExtent};
    use approx::assert_abs_diff_eq;

    fn point(x: f32, z: f32, width: f32) -> WaterPathPoint {
        WaterPathPoint {
            position: vec2(x, z),
            width,
        }
    }

    /// A straight river 4 wide and 10 long along +Z.
    fn straight() -> WaterPath {
        WaterPath {
            points: vec![point(0.0, 0.0, 4.0), point(0.0, 10.0, 4.0)],
            cell_size: 1.0,
            obstacles: Vec::new(),
            openings: Vec::new(),
        }
    }

    /// A river that bends left and right, narrowing and widening.
    fn meander() -> WaterPath {
        WaterPath {
            points: vec![
                point(0.0, 0.0, 6.0),
                point(5.0, 12.0, 8.0),
                point(-3.0, 24.0, 5.0),
                point(2.0, 36.0, 7.0),
            ],
            cell_size: 0.8,
            obstacles: vec![WaterCircle {
                center: vec2(5.0, 12.0),
                radius: 1.0,
            }],
            openings: Vec::new(),
        }
    }

    /// Twice the signed area of a triangle seen from above, in the X, Z
    /// plane: positive when it runs counter-clockwise.
    fn winding(a: Vec3, b: Vec3, c: Vec3) -> f32 {
        let (u, v) = (b - a, c - a);
        u.x * v.z - u.z * v.x
    }

    #[test]
    fn a_path_needs_two_points_a_width_and_a_cell_size() {
        assert!(straight().is_valid());
        let mut path = straight();
        path.points.pop();
        assert!(!path.is_valid());
        let mut path = straight();
        path.points[1].width = 0.0;
        assert!(path.geometry().is_none());
        let mut path = straight();
        path.cell_size = 0.0;
        assert!(path.sections().is_none());
        let mut path = straight();
        path.points[1].position = path.points[0].position;
        assert!(!path.is_valid());
    }

    #[test]
    fn a_straight_river_meshes_rows_along_it_and_columns_across() {
        let geometry = straight().geometry().unwrap();
        // 10 rows along, 4 columns across, two triangles of three vertices.
        let vertices = 10 * 4 * 6;
        assert_eq!(geometry.positions.len(), vertices);
        assert_eq!(geometry.indices.len(), vertices);
        for len in [
            geometry.river.len(),
            geometry.next.len(),
            geometry.previous.len(),
            geometry.next_river.len(),
            geometry.extra.len(),
            geometry.facets.len(),
        ] {
            assert_eq!(len, vertices);
        }
        assert!(geometry
            .indices
            .iter()
            .enumerate()
            .all(|(i, index)| *index as usize == i));
        for (position, river) in geometry.positions.iter().zip(&geometry.river) {
            assert_eq!(position.y, 0.0);
            // Flowing along +Z, the left of the flow is -X.
            assert_abs_diff_eq!(river.x, position.z, epsilon = 1e-4);
            assert_abs_diff_eq!(river.y, -position.x, epsilon = 1e-4);
            assert!(position.x.abs() <= 2.0 + 1e-4);
        }
    }

    #[test]
    fn triangles_face_up_as_a_heightfields_do() {
        let geometry = meander().geometry().unwrap();
        for corners in geometry.positions.chunks(3) {
            assert!(winding(corners[0], corners[1], corners[2]) > 0.0);
        }
        let field = Heightfield::new(2, 2, vec![0.0; 4]).unwrap();
        let terrain = field
            .geometry(&HeightfieldExtent {
                size: vec2(1.0, 1.0),
                height_scale: 1.0,
            })
            .unwrap();
        let corner = |i: usize| terrain.positions[terrain.indices[i] as usize];
        assert!(winding(corner(0), corner(1), corner(2)) > 0.0);
    }

    #[test]
    fn each_vertex_knows_its_triangles_other_corners_in_drawn_order() {
        let geometry = meander().geometry().unwrap();
        let xz = |p: Vec3| vec2(p.x, p.z);
        for t in 0..geometry.positions.len() / 3 {
            for i in 0..3 {
                let vertex = t * 3 + i;
                let next = t * 3 + (i + 1) % 3;
                let previous = t * 3 + (i + 2) % 3;
                assert_eq!(geometry.next[vertex], xz(geometry.positions[next]));
                assert_eq!(geometry.previous[vertex], xz(geometry.positions[previous]));
                assert_eq!(geometry.next_river[vertex], geometry.river[next]);
                let extra = geometry.extra[vertex];
                assert_eq!(vec2(extra.x, extra.y), geometry.river[previous]);
                assert_eq!(geometry.facets[vertex], geometry.facets[t * 3]);
            }
        }
    }

    /// Where a river opens into other water, its shore there is open
    /// water: no bank is cut along it, and the water there lies as far from
    /// the shore as the nearest stretch of shore that meets land, so no
    /// shallows or foam band draws across the mouth.
    #[test]
    fn an_opening_leaves_its_shore_without_a_bank_or_a_shore() {
        let profile = BankProfile {
            height: 0.6,
            depth: 0.6,
            run: 0.4,
            lip: 0.5,
        };
        let mut path = straight();
        let whole = path.banks(&profile).unwrap().positions.len();
        // An opening on the left shore (-X) halfway along.
        path.openings.push(WaterCircle {
            center: vec2(-2.0, 5.0),
            radius: 2.0,
        });
        let banks = path.banks(&profile).unwrap();
        assert!(banks.positions.len() < whole);
        assert!(banks
            .positions
            .iter()
            .all(|p| p.x > 0.0 || (p.z - 5.0).abs() >= 1.0 - 1e-4));
        let geometry = path.geometry().unwrap();
        let mut across_the_mouth = 0;
        for (position, extra) in geometry.positions.iter().zip(&geometry.extra) {
            let own = 2.0 - position.x.abs();
            let open = position.x < 0.0 && (position.z - 5.0).abs() < 2.0;
            if !open {
                assert_abs_diff_eq!(extra.z, own, epsilon = 1e-4);
            } else {
                // Never nearer the shore than its own, and well clear of it
                // in the mouth's middle, where the nearest land is the
                // mouth's ends or the far shore.
                assert!(extra.z >= own - 1e-4);
                if (position.z - 5.0).abs() < 0.5 {
                    across_the_mouth += 1;
                    assert!(extra.z > 1.4, "{position:?} is {} from the shore", extra.z);
                }
            }
        }
        assert!(across_the_mouth > 0);
    }

    #[test]
    fn shore_distance_runs_from_zero_at_the_banks_to_half_the_width() {
        let geometry = straight().geometry().unwrap();
        for (river, extra) in geometry.river.iter().zip(&geometry.extra) {
            assert_abs_diff_eq!(extra.z, 2.0 - river.y.abs(), epsilon = 1e-4);
        }
        let shores: Vec<f32> = geometry.extra.iter().map(|extra| extra.z).collect();
        assert_eq!(shores.iter().copied().fold(f32::MAX, f32::min), 0.0);
        assert!(shores.iter().copied().fold(0.0, f32::max) <= 2.0);
        for (corners, facet) in geometry
            .extra
            .chunks(3)
            .zip(geometry.facets.iter().step_by(3))
        {
            let centroid = (corners[0].z + corners[1].z + corners[2].z) / 3.0;
            assert_abs_diff_eq!(facet.y, centroid, epsilon = 1e-5);
            assert!((0.0..1.0).contains(&facet.x));
        }
    }

    #[test]
    fn a_meander_follows_its_points_and_widths() {
        let path = meander();
        let sections = path.sections().unwrap();
        // Through every control point, as wide as it is there.
        for control in &path.points {
            let nearest = sections
                .iter()
                .min_by(|a, b| {
                    (a.center - control.position)
                        .norm()
                        .total_cmp(&(b.center - control.position).norm())
                })
                .unwrap();
            assert!((nearest.center - control.position).norm() < path.cell_size);
            assert_abs_diff_eq!(nearest.half_width * 2.0, control.width, epsilon = 0.3);
        }
        // Rows are evenly spaced along the curve and flow from start to end.
        let step = sections[1].s;
        assert!((step - path.cell_size).abs() < 0.1);
        for pair in sections.windows(2) {
            assert_abs_diff_eq!(pair[1].s - pair[0].s, step, epsilon = 1e-3);
            assert!((pair[1].center - pair[0].center).norm() <= step + 1e-3);
            assert_abs_diff_eq!(pair[0].tangent.norm(), 1.0, epsilon = 1e-4);
        }
        // It bends one way, then the other.
        let turns: Vec<f32> = sections
            .windows(2)
            .map(|pair| {
                pair[0].tangent.x * pair[1].tangent.y - pair[0].tangent.y * pair[1].tangent.x
            })
            .collect();
        assert!(turns.iter().any(|turn| *turn > 1e-3));
        assert!(turns.iter().any(|turn| *turn < -1e-3));
    }

    #[test]
    fn obstacle_distance_is_to_the_nearest_circles_edge() {
        let path = meander();
        assert_eq!(path.obstacle_distance(vec2(5.0, 12.5)), 0.0);
        assert_abs_diff_eq!(path.obstacle_distance(vec2(8.0, 12.0)), 2.0, epsilon = 1e-5);
        assert_eq!(
            straight().obstacle_distance(vec2(0.0, 0.0)),
            MAX_OBSTACLE_DISTANCE
        );
        let geometry = path.geometry().unwrap();
        assert!(geometry.extra.iter().any(|extra| extra.w < 0.5));
    }

    #[test]
    fn banks_rise_from_under_the_water_to_a_lip_on_both_shores() {
        let profile = BankProfile {
            height: 0.6,
            depth: 0.4,
            run: 0.5,
            lip: 0.5,
        };
        let banks = straight().banks(&profile).unwrap();
        // 10 rows, 2 strips (wall and lip), 2 sides, 2 triangles of 3.
        assert_eq!(banks.positions.len(), 10 * 2 * 2 * 6);
        assert_eq!(banks.normals.len(), banks.positions.len());
        let ys: Vec<f32> = banks.positions.iter().map(|p| p.y).collect();
        assert_abs_diff_eq!(ys.iter().copied().fold(f32::MAX, f32::min), -0.4);
        assert_abs_diff_eq!(ys.iter().copied().fold(f32::MIN, f32::max), 0.6);
        // The wall's foot is inside the shore, X = +-2, its top outside.
        let xs: Vec<f32> = banks.positions.iter().map(|p| p.x.abs()).collect();
        assert!(xs.iter().copied().fold(f32::MAX, f32::min) < 2.0);
        assert_abs_diff_eq!(
            xs.iter().copied().fold(0.0, f32::max),
            2.0 + 0.3 + 0.5,
            epsilon = 1e-4
        );
        for (corners, normals) in banks.positions.chunks(3).zip(banks.normals.chunks(3)) {
            let normal = normals[0];
            // Every face looks up, and a wall towards the river (X = 0).
            assert!(normal.y > 0.0);
            let center = (corners[0] + corners[1] + corners[2]) / 3.0;
            assert!(normal.x * center.x <= 1e-6);
            // Its winding agrees with its normal, as the terrain's does.
            let facing = (corners[2] - corners[0]).cross(&(corners[1] - corners[0]));
            assert!(facing.dot(&normal) > 0.0);
        }
        assert!(straight()
            .banks(&BankProfile {
                height: 0.0,
                depth: 0.0,
                ..profile
            })
            .is_none());
    }
}
