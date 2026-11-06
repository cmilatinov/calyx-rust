//! Darkens the ground the player's side cannot currently see.
//!
//! The fog is a vertex-coloured grid laid on the ground under the view and
//! projected through the main camera. Each vertex is as dark as the side's
//! [`TeamVision`] leaves it, from the same sight rays and cover the minimap
//! lights, so ground in sight keeps its look and the fog edge is soft along
//! the vision range and every cover shadow.

use super::paint::{Painter, BLACK};
use super::Projection;
use crate::vision::{SightRays, TeamVision};
use calyx_engine::scene::Scene;
use calyx_engine::ui::UiColor;
use nalgebra_glm::{vec2, vec4, Vec2, Vec3};

/// Sight rays per tank; a cover shadow's edge is about one ray wide.
const RAYS: usize = 720;
/// How far the fog takes to reach full darkness past the edge of sight, in
/// metres.
const SOFT_EDGE: f32 = 1.4;
/// Opacity of the fog over unseen ground.
const DARKNESS: f32 = 0.64;
/// Most grid cells along either side of the ground near the side's tanks.
const MAX_CELLS: f32 = 160.0;
/// Smallest grid cell, in metres.
const MIN_CELL: f32 = 0.2;
/// Farthest the fog reaches from the middle of the view, in metres, for a
/// camera that sees the horizon.
const MAX_REACH: f32 = 90.0;

/// Paints fog over the ground at `ground_height` that `vision` does not see,
/// in a view of `width` by `height` painter units.
pub(super) fn paint_fog(
    p: &mut Painter,
    scene: &Scene,
    vision: &TeamVision,
    ground_height: f32,
    width: f32,
    height: f32,
) {
    let Some(projection) = Projection::scene(scene, width, height) else {
        return;
    };
    let Some((min, max)) = ground_under_view(&projection, ground_height) else {
        return;
    };
    let project = |point: Vec2| projection.point(Vec3::new(point.x, ground_height, point.y));
    let rays = vision.sight_rays(RAYS);
    // Only ground near a seeing tank needs the fine grid; everything else
    // under the view is fully dark, as flat quads around it.
    let near = vision
        .viewers
        .iter()
        .map(|viewer| {
            let reach = viewer.range + SOFT_EDGE * 2.0 + 2.0;
            (
                viewer.position - vec2(reach, reach),
                viewer.position + vec2(reach, reach),
            )
        })
        .reduce(|a, b| (a.0.inf(&b.0), a.1.sup(&b.1)))
        .map(|(a, b)| (a.sup(&min), b.inf(&max)))
        .filter(|(a, b)| a.x < b.x && a.y < b.y);
    let mut mesh = FogMesh::default();
    match near {
        Some((fine_min, fine_max)) => {
            mesh = fog_mesh(&rays, fine_min, fine_max, project);
            for (a, b) in [
                (min, vec2(max.x, fine_min.y)),
                (vec2(min.x, fine_max.y), max),
                (vec2(min.x, fine_min.y), vec2(fine_min.x, fine_max.y)),
                (vec2(fine_max.x, fine_min.y), vec2(max.x, fine_max.y)),
            ] {
                mesh.dark_quad(a, b, project);
            }
        }
        None => mesh.dark_quad(min, max, project),
    }
    if !mesh.indices.is_empty() {
        p.gradient(&mesh.points, &mesh.colors, &mesh.indices);
    }
}

/// A vertex-coloured triangle mesh in painter units.
#[derive(Default)]
struct FogMesh {
    points: Vec<(f32, f32)>,
    colors: Vec<UiColor>,
    indices: Vec<u32>,
}

impl FogMesh {
    /// Adds the ground rectangle `min`..`max` at full darkness, when it has
    /// area and all of it is on screen.
    fn dark_quad(&mut self, min: Vec2, max: Vec2, project: impl Fn(Vec2) -> Option<(f32, f32)>) {
        if min.x >= max.x || min.y >= max.y {
            return;
        }
        let corners = [min, vec2(max.x, min.y), max, vec2(min.x, max.y)].map(&project);
        if corners.iter().any(Option::is_none) {
            return;
        }
        let first = self.points.len() as u32;
        for corner in corners.into_iter().flatten() {
            self.points.push(corner);
            self.colors.push(BLACK.with_alpha(DARKNESS));
        }
        self.indices
            .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// How dark fog is at `point`, from 0 (in sight) to [`DARKNESS`].
fn darkness(rays: &[SightRays], point: Vec2) -> f32 {
    DARKNESS * (1.0 - TeamVision::coverage(rays, point, SOFT_EDGE))
}

/// The fog grid over the ground rectangle `min`..`max`, projected to the
/// screen by `project`. Cells with no fog, or a corner off screen, are left
/// out.
fn fog_mesh(
    rays: &[SightRays],
    min: Vec2,
    max: Vec2,
    project: impl Fn(Vec2) -> Option<(f32, f32)>,
) -> FogMesh {
    let size = max - min;
    let cell = (size.x.max(size.y) / MAX_CELLS).max(MIN_CELL);
    let columns = (size.x / cell).ceil().max(1.0) as usize;
    let rows = (size.y / cell).ceil().max(1.0) as usize;
    // Fit the cells to the rectangle exactly, so its edges meet the dark
    // quads around it without overlap.
    let step = vec2(size.x / columns as f32, size.y / rows as f32);
    let mut mesh = FogMesh::default();
    let mut on_screen = Vec::with_capacity((columns + 1) * (rows + 1));
    let mut alpha = Vec::with_capacity((columns + 1) * (rows + 1));
    for row in 0..=rows {
        for column in 0..=columns {
            let point = min + vec2(column as f32 * step.x, row as f32 * step.y);
            let dark = darkness(rays, point);
            let screen = project(point);
            on_screen.push(screen.is_some());
            alpha.push(dark);
            mesh.points.push(screen.unwrap_or_default());
            mesh.colors.push(BLACK.with_alpha(dark));
        }
    }
    let at = |row: usize, column: usize| row * (columns + 1) + column;
    for row in 0..rows {
        for column in 0..columns {
            let corners = [
                at(row, column),
                at(row, column + 1),
                at(row + 1, column + 1),
                at(row + 1, column),
            ];
            if corners.iter().any(|&i| !on_screen[i]) || corners.iter().all(|&i| alpha[i] <= 0.0) {
                continue;
            }
            let [a, b, c, d] = corners.map(|i| i as u32);
            mesh.indices.extend([a, b, c, a, c, d]);
        }
    }
    mesh
}

/// The ground rectangle, in `(x, z)`, that the view sees at `ground_height`.
fn ground_under_view(projection: &Projection, ground_height: f32) -> Option<(Vec2, Vec2)> {
    let inverse = projection.matrix.try_inverse()?;
    let unproject = |x: f32, y: f32, depth: f32| {
        let point = inverse * vec4(x, y, depth, 1.0);
        (point.w.abs() > f32::EPSILON).then(|| point.xyz() / point.w)
    };
    let centre = unproject(0.0, 0.0, 0.0)
        .zip(unproject(0.0, 0.0, 1.0))
        .and_then(|(near, far)| hit_ground(near, far, ground_height))
        .unwrap_or_default();
    let mut min = vec2(f32::INFINITY, f32::INFINITY);
    let mut max = vec2(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for (x, y) in [
        (-1.0, -1.0),
        (1.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (0.0, -1.0),
    ] {
        let (Some(near), Some(far)) = (unproject(x, y, 0.0), unproject(x, y, 1.0)) else {
            continue;
        };
        // A corner above the horizon sees fog as far as fog reaches.
        let point = hit_ground(near, far, ground_height).unwrap_or_else(|| {
            let direction = vec2(far.x - near.x, far.z - near.z);
            centre + direction.try_normalize(f32::EPSILON).unwrap_or_default() * MAX_REACH
        });
        let point = centre + (point - centre).cap_magnitude(MAX_REACH);
        min = min.inf(&point);
        max = max.sup(&point);
    }
    (min.x < max.x && min.y < max.y).then(|| {
        // One cell of margin keeps the screen edge covered.
        let margin = vec2(1.0, 1.0);
        (min - margin, max + margin)
    })
}

/// Where the line from `near` to `far` meets the ground, beyond `near`.
fn hit_ground(near: Vec3, far: Vec3, ground_height: f32) -> Option<Vec2> {
    let rise = far.y - near.y;
    if rise.abs() <= f32::EPSILON {
        return None;
    }
    let t = (ground_height - near.y) / rise;
    (t >= 0.0).then(|| {
        let point = near + (far - near) * t;
        vec2(point.x, point.z)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::{GroundRect, Viewer};

    fn vision(cover: Vec<GroundRect>) -> TeamVision {
        TeamVision {
            viewers: vec![Viewer {
                position: Vec2::zeros(),
                range: 10.0,
            }],
            cover,
        }
    }

    #[test]
    fn fog_is_clear_in_sight_and_dark_past_range_and_behind_cover() {
        let vision = vision(vec![GroundRect::axis_aligned(
            vec2(4.0, 0.0),
            vec2(2.0, 4.0),
        )]);
        let rays = vision.sight_rays(RAYS);
        assert_eq!(darkness(&rays, vec2(2.0, 0.0)), 0.0);
        assert_eq!(darkness(&rays, vec2(0.0, -9.0)), 0.0);
        // The cover block itself stays clear; its shadow is dark.
        assert_eq!(darkness(&rays, vec2(4.5, 0.0)), 0.0);
        assert_eq!(darkness(&rays, vec2(8.0, 0.0)), DARKNESS);
        assert_eq!(darkness(&rays, vec2(0.0, 12.0)), DARKNESS);
        // The edge is soft: partway dark just past the range.
        let edge = darkness(&rays, vec2(0.0, 10.0 + SOFT_EDGE * 0.5));
        assert!(edge > 0.0 && edge < DARKNESS, "{edge}");
    }

    #[test]
    fn fog_mesh_leaves_out_clear_and_off_screen_cells() {
        let rays = vision(Vec::new()).sight_rays(RAYS);
        let clear = fog_mesh(&rays, vec2(-2.0, -2.0), vec2(2.0, 2.0), |p| {
            Some((p.x, p.y))
        });
        assert!(clear.indices.is_empty());
        let dark = fog_mesh(&rays, vec2(20.0, 20.0), vec2(24.0, 24.0), |p| {
            Some((p.x, p.y))
        });
        assert!(!dark.indices.is_empty());
        assert!(dark.colors.iter().all(|c| c.a > 0));
        let hidden = fog_mesh(&rays, vec2(20.0, 20.0), vec2(24.0, 24.0), |_| None);
        assert!(hidden.indices.is_empty());
    }

    #[test]
    fn no_seeing_tank_darkens_everything() {
        let rays = TeamVision::default().sight_rays(RAYS);
        assert_eq!(darkness(&rays, Vec2::zeros()), DARKNESS);
    }
}
