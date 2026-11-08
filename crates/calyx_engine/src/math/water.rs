use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

/// Most cells a [`WaterGrid`] meshes along either side, so a water surface
/// stays within about 1.6 million vertices.
pub const MAX_WATER_CELLS: u32 = 512;

/// How far, as a share of a cell, the grid moves each inner vertex off its
/// regular position, so the facets come out irregular rather than as a
/// ruled grid.
const VERTEX_JITTER: f32 = 0.3;

/// The faceted surface of a body of water: a local XZ rectangle of `size`,
/// centered on the origin, cut into `columns` by `rows` cells of two
/// triangles each.
///
/// The surface lies on local Y = 0; the water shader moves its vertices in a
/// wave. Column 0 sits on the -X edge and row 0 on the +Z edge, as in a
/// [`Heightfield`](crate::math::Heightfield). Inner vertices are moved off
/// the regular grid by a fixed pseudo-random amount, and the cells alternate
/// their diagonal, so the facets are irregular. Vertices on an edge only
/// move along it, so the rectangle keeps its outline.
///
/// The shore is the rectangle's outline: the banks a scene builds the water
/// into run along its edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaterGrid {
    /// Width along X and depth along Z.
    pub size: Vec2,
    /// Cells along X.
    pub columns: u32,
    /// Cells along Z.
    pub rows: u32,
}

/// The triangle mesh of a [`WaterGrid`], three vertices of its own per
/// triangle, so each facet carries what the water shader needs to move it
/// and shade it flat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WaterGeometry {
    /// Local rest positions, all on Y = 0.
    pub positions: Vec<Vec3>,
    /// The rest normal, +Y.
    pub normals: Vec<Vec3>,
    /// Per vertex, the same for all three of a triangle's: X is the distance
    /// from the triangle's centroid to the shore, in local units; Y is a
    /// pseudo-random value in `[0, 1)` for the facet.
    pub facets: Vec<Vec2>,
    /// The local XZ rest position of the triangle's next corner, in drawn
    /// order.
    pub next: Vec<Vec2>,
    /// The local XZ rest position of the triangle's previous corner.
    pub previous: Vec<Vec2>,
    /// Triangle list indices, `0..positions.len()` in order.
    pub indices: Vec<u32>,
}

impl WaterGrid {
    /// Whether the grid meshes a surface: a finite, positive size and from 1
    /// to [`MAX_WATER_CELLS`] cells on each side.
    pub fn is_valid(&self) -> bool {
        let cells = 1..=MAX_WATER_CELLS;
        self.size.x.is_finite()
            && self.size.y.is_finite()
            && self.size.x > 0.0
            && self.size.y > 0.0
            && cells.contains(&self.columns)
            && cells.contains(&self.rows)
    }

    /// The distance from local `point` (X, Z) to the shore, the nearest edge
    /// of the rectangle; 0 outside it.
    pub fn shore_distance(&self, point: Vec2) -> f32 {
        let half = self.size * 0.5;
        let x = half.x - point.x.abs();
        let z = half.y - point.y.abs();
        x.min(z).max(0.0)
    }

    /// The local XZ rest position of the grid vertex at `column` and `row`.
    pub fn vertex(&self, column: u32, row: u32) -> Vec2 {
        let cell = vec2(
            self.size.x / self.columns as f32,
            self.size.y / self.rows as f32,
        );
        let mut position = vec2(
            column as f32 * cell.x - self.size.x * 0.5,
            self.size.y * 0.5 - row as f32 * cell.y,
        );
        let jitter = vec2(
            hash01(column, row, 0x9e37) - 0.5,
            hash01(column, row, 0x85eb) - 0.5,
        ) * (2.0 * VERTEX_JITTER);
        if column != 0 && column != self.columns {
            position.x += jitter.x * cell.x;
        }
        if row != 0 && row != self.rows {
            position.y += jitter.y * cell.y;
        }
        position
    }

    /// The grid's triangle mesh, or `None` when the grid is not valid.
    ///
    /// Each triangle runs counter-clockwise seen from above, as a
    /// heightfield's do, so the upper side faces outward.
    pub fn geometry(&self) -> Option<WaterGeometry> {
        if !self.is_valid() {
            return None;
        }
        let count = self.columns as usize * self.rows as usize * 6;
        let mut geometry = WaterGeometry {
            positions: Vec::with_capacity(count),
            normals: Vec::with_capacity(count),
            facets: Vec::with_capacity(count),
            next: Vec::with_capacity(count),
            previous: Vec::with_capacity(count),
            indices: Vec::with_capacity(count),
        };
        let mut triangle = |corners: [Vec2; 3], index: u32| {
            let centroid = (corners[0] + corners[1] + corners[2]) / 3.0;
            let facet = vec2(self.shore_distance(centroid), hash01(index, 0, 0x2c1b));
            for i in 0..3 {
                let corner = corners[i];
                geometry.indices.push(geometry.positions.len() as u32);
                geometry.positions.push(vec3(corner.x, 0.0, corner.y));
                geometry.normals.push(Vec3::y());
                geometry.facets.push(facet);
                geometry.next.push(corners[(i + 1) % 3]);
                geometry.previous.push(corners[(i + 2) % 3]);
            }
        };
        let mut index = 0;
        for row in 0..self.rows {
            for column in 0..self.columns {
                // a at (column, row), b one column on, c one row on (toward
                // -Z), d both.
                let a = self.vertex(column, row);
                let b = self.vertex(column + 1, row);
                let c = self.vertex(column, row + 1);
                let d = self.vertex(column + 1, row + 1);
                // Alternating diagonals break the grid into diamonds.
                let split = if (column + row) % 2 == 0 {
                    [[a, c, b], [b, c, d]]
                } else {
                    [[a, c, d], [a, d, b]]
                };
                for corners in split {
                    triangle(corners, index);
                    index += 1;
                }
            }
        }
        Some(geometry)
    }
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

    fn grid(columns: u32, rows: u32) -> WaterGrid {
        WaterGrid {
            size: vec2(8.0, 4.0),
            columns,
            rows,
        }
    }

    /// Twice the signed area of a triangle seen from above, in the X, Z
    /// plane: positive when it runs counter-clockwise.
    fn winding(a: Vec3, b: Vec3, c: Vec3) -> f32 {
        let (u, v) = (b - a, c - a);
        u.x * v.z - u.z * v.x
    }

    #[test]
    fn a_grid_needs_a_positive_size_and_cells_on_both_sides() {
        assert!(grid(4, 2).is_valid());
        assert!(!grid(0, 2).is_valid());
        assert!(!grid(4, MAX_WATER_CELLS + 1).is_valid());
        let mut flat = grid(4, 2);
        flat.size.y = 0.0;
        assert!(!flat.is_valid());
        flat.size.y = f32::NAN;
        assert!(flat.geometry().is_none());
    }

    #[test]
    fn every_cell_is_two_triangles_of_three_own_vertices() {
        let geometry = grid(5, 3).geometry().unwrap();
        let triangles = 5 * 3 * 2;
        assert_eq!(geometry.indices.len(), triangles * 3);
        assert_eq!(geometry.positions.len(), triangles * 3);
        for attribute in [
            geometry.normals.len(),
            geometry.facets.len(),
            geometry.next.len(),
            geometry.previous.len(),
        ] {
            assert_eq!(attribute, triangles * 3);
        }
        assert!(geometry
            .indices
            .iter()
            .enumerate()
            .all(|(i, index)| *index as usize == i));
        assert!(geometry.positions.iter().all(|p| p.y == 0.0));
        assert!(geometry.normals.iter().all(|n| *n == Vec3::y()));
    }

    #[test]
    fn triangles_face_up_as_a_heightfields_do() {
        let geometry = grid(6, 4).geometry().unwrap();
        for corners in geometry.positions.chunks(3) {
            assert!(winding(corners[0], corners[1], corners[2]) > 0.0);
        }
        // The terrain's triangles, which draw their upper side, run the same way.
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
        let geometry = grid(3, 3).geometry().unwrap();
        let xz = |p: Vec3| vec2(p.x, p.z);
        for (t, corners) in geometry.positions.chunks(3).enumerate() {
            for i in 0..3 {
                let vertex = t * 3 + i;
                assert_eq!(geometry.next[vertex], xz(corners[(i + 1) % 3]));
                assert_eq!(geometry.previous[vertex], xz(corners[(i + 2) % 3]));
                assert_eq!(geometry.facets[vertex], geometry.facets[t * 3]);
            }
        }
    }

    #[test]
    fn the_surface_covers_its_rectangle_and_keeps_its_outline() {
        let water = grid(8, 4);
        let geometry = water.geometry().unwrap();
        let area: f32 = geometry
            .positions
            .chunks(3)
            .map(|c| winding(c[0], c[1], c[2]) * 0.5)
            .sum();
        assert_abs_diff_eq!(area, 32.0, epsilon = 1e-3);
        for p in &geometry.positions {
            assert!(p.x.abs() <= 4.0 + 1e-5 && p.z.abs() <= 2.0 + 1e-5);
        }
        // Inner vertices move off the regular grid, edge ones only along it.
        assert_ne!(water.vertex(3, 2), vec2(-1.0, 0.0));
        assert_eq!(water.vertex(0, 2).x, -4.0);
        assert_eq!(water.vertex(3, 0).y, 2.0);
        assert_eq!(water.vertex(8, 4), vec2(4.0, -2.0));
        assert_eq!(
            water.vertex(3, 2),
            grid(8, 4).vertex(3, 2),
            "the layout is fixed"
        );
    }

    #[test]
    fn shore_distance_is_to_the_nearest_edge_at_each_facets_centroid() {
        let water = grid(8, 4);
        assert_eq!(water.shore_distance(vec2(0.0, 0.0)), 2.0);
        assert_eq!(water.shore_distance(vec2(3.5, 0.0)), 0.5);
        assert_eq!(water.shore_distance(vec2(-1.0, -1.25)), 0.75);
        assert_eq!(water.shore_distance(vec2(9.0, 0.0)), 0.0);
        let geometry = water.geometry().unwrap();
        for (corners, facet) in geometry
            .positions
            .chunks(3)
            .zip(geometry.facets.iter().step_by(3))
        {
            let centroid = (corners[0] + corners[1] + corners[2]) / 3.0;
            assert_abs_diff_eq!(
                facet.x,
                water.shore_distance(vec2(centroid.x, centroid.z)),
                epsilon = 1e-6
            );
            assert!((0.0..1.0).contains(&facet.y));
        }
        // The facets along the edge are the closest to the shore; the middle
        // row's are the deepest.
        let nearest = geometry.facets.iter().map(|f| f.x).fold(f32::MAX, f32::min);
        let deepest = geometry.facets.iter().map(|f| f.x).fold(0.0, f32::max);
        assert!(nearest < 0.25, "{nearest}");
        assert!(deepest > 1.25 && deepest <= 2.0, "{deepest}");
    }
}
