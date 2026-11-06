//! Darkens the ground the player's side cannot currently see.
//!
//! The fog is a vertex-coloured grid laid on the ground under the view and
//! projected through the main camera. Each vertex is as dark as the side's
//! [`TeamVision`] leaves it, from the same sight rays and cover the minimap
//! lights. That darkness is smooth by construction: it falls off across a
//! soft edge several grid cells wide, both at the end of sight and across
//! every cover shadow, so linear interpolation between vertices draws it
//! without blocks or steps. The grid's vertices sit on a fixed lattice in
//! the world, so they do not swim as the view moves, and each one eases
//! towards its new darkness over [`FADE_TIME`] instead of jumping.

use super::paint::{Painter, BLACK};
use super::Projection;
use crate::vision::{SightRays, TeamVision, FADE_TIME};
use calyx_engine::scene::Scene;
use calyx_engine::ui::UiColor;
use nalgebra_glm::{vec2, vec4, Vec2, Vec3};

/// Sight rays per tank; a cover shadow's side lies between two rays.
const RAYS: usize = 720;
/// How wide the fog's soft edge is, in metres: the end of sight and the
/// sides of cover shadows fade over this much ground.
const SOFT_EDGE: f32 = 3.0;
/// Opacity of the fog over unseen ground.
const DARKNESS: f32 = 0.64;
/// Most grid cells along either side of the ground near the side's tanks.
const MAX_CELLS: f32 = 128.0;
/// Smallest grid cell, in metres.
const MIN_CELL: f32 = 0.2;
/// Grid cells are a whole number of these, in metres, so the lattice only
/// changes when the ground near the tanks changes size by as much.
const CELL_STEP: f32 = 0.05;
/// Farthest the fog reaches from the middle of the view, in metres, for a
/// camera that sees the horizon.
const MAX_REACH: f32 = 90.0;
/// Longest step of fog time one paint lets pass, in seconds, so a long
/// pause does not count as fading.
const MAX_STEP: f32 = 0.1;

/// The fog darkness painted last time, so the next paint can ease from it.
#[derive(Debug, Default)]
pub(crate) struct FogFade {
    /// The grid painted last time, if any.
    grid: Option<FogGrid>,
    /// The ground under the whole view last time; past the grid it was
    /// fully dark.
    view: Option<(Vec2, Vec2)>,
    /// The HUD time of the last paint, in seconds.
    time: Option<f32>,
}

/// Fog darkness on a lattice of ground points.
#[derive(Clone, Debug)]
struct FogGrid {
    /// Lattice coordinates of the first vertex.
    origin: [i64; 2],
    cell: f32,
    /// Vertices along X and Z.
    columns: usize,
    rows: usize,
    darkness: Vec<f32>,
}

impl FogGrid {
    fn point(&self, column: usize, row: usize) -> Vec2 {
        vec2(
            (self.origin[0] + column as i64) as f32 * self.cell,
            (self.origin[1] + row as i64) as f32 * self.cell,
        )
    }

    fn min(&self) -> Vec2 {
        self.point(0, 0)
    }

    fn max(&self) -> Vec2 {
        self.point(self.columns - 1, self.rows - 1)
    }

    /// The darkness at `point`, interpolated between the four nearest
    /// vertices, or `None` off the grid.
    fn sample(&self, point: Vec2) -> Option<f32> {
        const SLACK: f32 = 1e-3;
        let x = point.x / self.cell - self.origin[0] as f32;
        let y = point.y / self.cell - self.origin[1] as f32;
        let (last_x, last_y) = ((self.columns - 1) as f32, (self.rows - 1) as f32);
        if x < -SLACK || y < -SLACK || x > last_x + SLACK || y > last_y + SLACK {
            return None;
        }
        let (x, y) = (x.clamp(0.0, last_x), y.clamp(0.0, last_y));
        let column = (x.floor() as usize).min(self.columns.saturating_sub(2));
        let row = (y.floor() as usize).min(self.rows.saturating_sub(2));
        let next_column = (column + 1).min(self.columns - 1);
        let next_row = (row + 1).min(self.rows - 1);
        let (u, v) = (x - column as f32, y - row as f32);
        let at = |c: usize, r: usize| self.darkness[r * self.columns + c];
        let near = at(column, row) * (1.0 - u) + at(next_column, row) * u;
        let far = at(column, next_row) * (1.0 - u) + at(next_column, next_row) * u;
        Some(near * (1.0 - v) + far * v)
    }

    /// The ground around every vertex lighter than full fog, if any is.
    fn lit_bounds(&self) -> Option<(Vec2, Vec2)> {
        let mut lit = (0..self.rows)
            .flat_map(|row| (0..self.columns).map(move |column| (column, row)))
            .filter(|&(column, row)| self.darkness[row * self.columns + column] < DARKNESS)
            .map(|(column, row)| self.point(column, row));
        let first = lit.next()?;
        let (min, max) = lit.fold((first, first), |(min, max), p| (min.inf(&p), max.sup(&p)));
        let margin = vec2(self.cell, self.cell);
        Some((min - margin, max + margin))
    }
}

impl FogFade {
    /// How dark fog was at `point` last paint, if this fade remembers it.
    fn previous(&self, point: Vec2) -> Option<f32> {
        if let Some(dark) = self.grid.as_ref().and_then(|grid| grid.sample(point)) {
            return Some(dark);
        }
        let (min, max) = self.view?;
        (point.x >= min.x && point.y >= min.y && point.x <= max.x && point.y <= max.y)
            .then_some(DARKNESS)
    }

    /// How much fog time passes between the last paint and one at `time`.
    fn step(&mut self, time: f32) -> f32 {
        let dt = self
            .time
            .map_or(0.0, |last| (time - last).clamp(0.0, MAX_STEP));
        self.time = Some(time);
        dt
    }
}

/// Paints fog over the ground at `ground_height` that `vision` does not see,
/// in a view of `width` by `height` painter units, at HUD time `time`
/// seconds; `fade` carries the fog from one paint to the next.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_fog(
    p: &mut Painter,
    scene: &Scene,
    vision: &TeamVision,
    ground_height: f32,
    width: f32,
    height: f32,
    fade: &mut FogFade,
    time: f32,
) {
    let dt = fade.step(time);
    let Some(projection) = Projection::scene(scene, width, height) else {
        return;
    };
    let Some((min, max)) = ground_under_view(&projection, ground_height) else {
        return;
    };
    let project = |point: Vec2| projection.point(Vec3::new(point.x, ground_height, point.y));
    let rays = vision.sight_rays(RAYS);
    // Only ground near a seeing tank, or still fading, needs the fine grid;
    // everything else under the view is fully dark, as flat quads around it.
    let fading = fade.grid.as_ref().and_then(FogGrid::lit_bounds);
    let near = vision
        .viewers
        .iter()
        .map(|viewer| {
            let reach = viewer.range + SOFT_EDGE + 1.0;
            (
                viewer.position - vec2(reach, reach),
                viewer.position + vec2(reach, reach),
            )
        })
        .chain(fading)
        .reduce(|a, b| (a.0.inf(&b.0), a.1.sup(&b.1)))
        .map(|(a, b)| (a.sup(&min), b.inf(&max)))
        .filter(|(a, b)| a.x < b.x && a.y < b.y);
    let grid = near.map(|(fine_min, fine_max)| fog_grid(&rays, fine_min, fine_max, fade, dt));
    let mut mesh = match &grid {
        Some(grid) => {
            let mut mesh = fog_mesh(grid, project);
            let (fine_min, fine_max) = (grid.min(), grid.max());
            for (a, b) in [
                (min, vec2(max.x, fine_min.y)),
                (vec2(min.x, fine_max.y), max),
                (vec2(min.x, fine_min.y), vec2(fine_min.x, fine_max.y)),
                (vec2(fine_max.x, fine_min.y), vec2(max.x, fine_max.y)),
            ] {
                mesh.dark_quad(a, b, project);
            }
            mesh
        }
        None => FogMesh::default(),
    };
    if grid.is_none() {
        mesh.dark_quad(min, max, project);
    }
    fade.grid = grid;
    fade.view = Some((min, max));
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

/// The size of a grid cell over ground `size` across: as small as
/// [`MIN_CELL`], no more than [`MAX_CELLS`] of them, rounded up to whole
/// [`CELL_STEP`]s.
fn cell_size(size: Vec2) -> f32 {
    let cell = (size.x.max(size.y) / MAX_CELLS).max(MIN_CELL);
    (cell / CELL_STEP).ceil() * CELL_STEP
}

/// The fog darkness over the ground rectangle `min`..`max`, widened to whole
/// cells of the lattice. Each vertex eases from the darkness `fade`
/// remembers towards what `rays` leave it, by at most a full fade's share
/// of `dt` seconds.
fn fog_grid(rays: &[SightRays], min: Vec2, max: Vec2, fade: &FogFade, dt: f32) -> FogGrid {
    let cell = cell_size(max - min);
    let origin = [(min.x / cell).floor() as i64, (min.y / cell).floor() as i64];
    let end = [(max.x / cell).ceil() as i64, (max.y / cell).ceil() as i64];
    let columns = (end[0] - origin[0]).max(1) as usize + 1;
    let rows = (end[1] - origin[1]).max(1) as usize + 1;
    let mut grid = FogGrid {
        origin,
        cell,
        columns,
        rows,
        darkness: Vec::with_capacity(columns * rows),
    };
    let most = DARKNESS * dt / FADE_TIME;
    for row in 0..rows {
        for column in 0..columns {
            let point = grid.point(column, row);
            let target = darkness(rays, point);
            let dark = match fade.previous(point) {
                Some(previous) => previous + (target - previous).clamp(-most, most),
                None => target,
            };
            grid.darkness.push(dark);
        }
    }
    grid
}

/// The triangles of `grid`, projected to the screen by `project`. Cells with
/// no fog, or a corner off screen, are left out.
fn fog_mesh(grid: &FogGrid, project: impl Fn(Vec2) -> Option<(f32, f32)>) -> FogMesh {
    let (columns, rows) = (grid.columns, grid.rows);
    let mut mesh = FogMesh::default();
    let mut on_screen = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let screen = project(grid.point(column, row));
            on_screen.push(screen.is_some());
            mesh.points.push(screen.unwrap_or_default());
            mesh.colors
                .push(BLACK.with_alpha(grid.darkness[row * columns + column]));
        }
    }
    let at = |row: usize, column: usize| row * columns + column;
    for row in 0..rows - 1 {
        for column in 0..columns - 1 {
            let corners = [
                at(row, column),
                at(row, column + 1),
                at(row + 1, column + 1),
                at(row + 1, column),
            ];
            if corners.iter().any(|&i| !on_screen[i])
                || corners.iter().all(|&i| grid.darkness[i] <= 0.0)
            {
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

    /// A block of cover across +X, 3 to 5 m out and 4 m wide, whose shadow's
    /// upper side runs from the viewer through the block's corner at (3, 2).
    fn block() -> TeamVision {
        vision(vec![GroundRect::axis_aligned(
            vec2(4.0, 0.0),
            vec2(2.0, 4.0),
        )])
    }

    /// Darkness at `samples + 1` evenly spaced points from `from` to `to`.
    fn profile(rays: &[SightRays], from: Vec2, to: Vec2, samples: usize) -> Vec<f32> {
        (0..=samples)
            .map(|i| darkness(rays, from + (to - from) * (i as f32 / samples as f32)))
            .collect()
    }

    /// Asserts `values` fall from full fog to clear without ever rising, and
    /// change by no more than `most` between neighbours.
    fn assert_smooth_fall(values: &[f32], most: f32) {
        assert!(
            (values[0] - DARKNESS).abs() < 1e-3,
            "starts dark: {}",
            values[0]
        );
        assert!(values[values.len() - 1] < 1e-3, "ends clear");
        for pair in values.windows(2) {
            assert!(pair[1] <= pair[0] + 1e-4, "never darkens again: {pair:?}");
            assert!((pair[1] - pair[0]).abs() <= most, "no step: {pair:?}");
        }
    }

    /// Paints `rays` into `fade` at `time` seconds over the ground within
    /// 14 m of the origin, as one frame of [`paint_fog`] does.
    fn paint(fade: &mut FogFade, rays: &[SightRays], time: f32) {
        let dt = fade.step(time);
        let grid = fog_grid(rays, vec2(-14.0, -14.0), vec2(14.0, 14.0), fade, dt);
        fade.grid = Some(grid);
        fade.view = Some((vec2(-30.0, -30.0), vec2(30.0, 30.0)));
    }

    #[test]
    fn fog_is_clear_in_sight_and_dark_past_range_and_behind_cover() {
        let rays = block().sight_rays(RAYS);
        assert_eq!(darkness(&rays, vec2(2.0, 0.0)), 0.0);
        assert_eq!(darkness(&rays, vec2(0.0, -9.0)), 0.0);
        // The cover block itself stays nearly clear; its shadow is dark.
        assert!(darkness(&rays, vec2(4.5, 0.0)) < DARKNESS * 0.1);
        assert_eq!(darkness(&rays, vec2(8.0, 0.0)), DARKNESS);
        assert_eq!(darkness(&rays, vec2(0.0, 12.5)), DARKNESS);
        // The edge is soft: partway dark just past the range.
        let edge = darkness(&rays, vec2(0.0, 10.5));
        assert!(edge > 0.0 && edge < DARKNESS, "{edge}");
    }

    #[test]
    fn the_end_of_sight_fades_smoothly_over_the_soft_edge() {
        let rays = block().sight_rays(RAYS);
        // Straight out past the range, 1 cm at a time.
        let mut values = profile(&rays, vec2(0.0, 7.0), vec2(0.0, 13.0), 600);
        values.reverse();
        // A smoothstep over the soft edge is never steeper than 1.5 times
        // its average slope.
        let most = DARKNESS / SOFT_EDGE * 1.5 * 0.01 * 1.05;
        assert_smooth_fall(&values, most);
        let fading = values.iter().filter(|&&v| v > 0.01 && v < DARKNESS - 0.01);
        assert!(
            fading.count() as f32 * 0.01 > SOFT_EDGE * 0.7,
            "the fade is about as wide as the soft edge"
        );
    }

    #[test]
    fn a_cover_shadow_side_fades_smoothly_with_no_ray_steps() {
        let rays = block().sight_rays(RAYS);
        // Across the shadow's upper side, 8 m out, 1 cm at a time, from
        // inside the shadow to open ground.
        let side = 2.0f32.atan2(3.0);
        let along = vec2(side.cos(), side.sin()) * 8.0;
        let across = vec2(-side.sin(), side.cos());
        let values = profile(&rays, along - across * 3.0, along + across * 3.0, 600);
        // The window over rays peaks at 15/16 of a unit over half the soft
        // edge; allow for the sum over discrete rays.
        let most = DARKNESS * (15.0 / 16.0) / (SOFT_EDGE * 0.5) * 0.01 * 1.25;
        assert_smooth_fall(&values, most);
        let fading = values.iter().filter(|&&v| v > 0.01 && v < DARKNESS - 0.01);
        assert!(
            fading.count() as f32 * 0.01 > SOFT_EDGE * 0.5,
            "the side is as soft as the end of sight"
        );
    }

    #[test]
    fn the_grid_draws_the_fog_without_cell_sized_steps() {
        let rays = block().sight_rays(RAYS);
        let grid = fog_grid(
            &rays,
            vec2(-14.0, -14.0),
            vec2(14.0, 14.0),
            &FogFade::default(),
            0.0,
        );
        // Between vertices the painter blends linearly; in the middle of
        // every cell that blend must match the fog it stands for.
        let mut worst = 0.0f32;
        for row in 0..grid.rows - 1 {
            for column in 0..grid.columns - 1 {
                let middle = (grid.point(column, row) + grid.point(column + 1, row + 1)) * 0.5;
                let blended = grid.sample(middle).unwrap();
                worst = worst.max((blended - darkness(&rays, middle)).abs());
            }
        }
        assert!(worst < DARKNESS * 0.03, "worst cell error {worst}");
    }

    #[test]
    fn grid_vertices_sit_on_a_fixed_world_lattice() {
        let rays = block().sight_rays(RAYS);
        let fade = FogFade::default();
        let a = fog_grid(&rays, vec2(-14.0, -14.0), vec2(14.0, 14.0), &fade, 0.0);
        let b = fog_grid(&rays, vec2(-13.93, -14.07), vec2(14.07, 13.93), &fade, 0.0);
        assert_eq!(a.cell, b.cell);
        let lattice = |p: Vec2| (p / a.cell).map(|v| (v - v.round()).abs() < 1e-3);
        assert!(lattice(a.min()).iter().all(|&on| on));
        assert!(lattice(b.min()).iter().all(|&on| on));
        assert!(a.min().x <= -14.0 && a.max().x >= 14.0);
    }

    #[test]
    fn fog_eases_in_and_out_over_the_fade_time() {
        let seen = vision(Vec::new()).sight_rays(RAYS);
        let blind = TeamVision::default().sight_rays(RAYS);
        let mut fade = FogFade::default();
        let at = |fade: &FogFade| fade.grid.as_ref().unwrap().sample(Vec2::zeros()).unwrap();

        // The first paint shows the fog as it is.
        paint(&mut fade, &seen, 1.0);
        assert_eq!(at(&fade), 0.0);

        // Losing sight closes the fog over a third of the fade at a time.
        let tick = FADE_TIME / 3.0;
        paint(&mut fade, &blind, 1.0 + tick);
        assert!((at(&fade) - DARKNESS / 3.0).abs() < 1e-3, "{}", at(&fade));
        paint(&mut fade, &blind, 1.0 + tick * 2.0);
        assert!((at(&fade) - DARKNESS * 2.0 / 3.0).abs() < 1e-3);
        paint(&mut fade, &blind, 1.0 + tick * 3.0);
        assert!((at(&fade) - DARKNESS).abs() < 1e-3);

        // Regaining it clears the fog the same way.
        paint(&mut fade, &seen, 1.0 + tick * 4.0);
        assert!((at(&fade) - DARKNESS * 2.0 / 3.0).abs() < 1e-3);
        paint(&mut fade, &seen, 1.0 + tick * 5.0);
        paint(&mut fade, &seen, 1.0 + tick * 6.0);
        assert_eq!(at(&fade), 0.0);

        // A paint at the same time, as a second paint in one frame, changes
        // nothing.
        paint(&mut fade, &blind, 1.0 + tick * 6.0);
        assert_eq!(at(&fade), 0.0);
    }

    #[test]
    fn ground_coming_out_from_the_dark_quads_fades_in() {
        let seen = vision(Vec::new()).sight_rays(RAYS);
        // Last paint, the fine grid was far away, and the ground here lay
        // in the view's fully dark quads.
        let mut fade = FogFade {
            view: Some((vec2(-30.0, -30.0), vec2(30.0, 30.0))),
            time: Some(0.0),
            ..Default::default()
        };
        paint(&mut fade, &seen, 0.1);
        let dark = fade.grid.as_ref().unwrap().sample(Vec2::zeros()).unwrap();
        assert!(dark > DARKNESS * 0.5 && dark < DARKNESS, "{dark}");
    }

    #[test]
    fn fog_mesh_leaves_out_clear_and_off_screen_cells() {
        let rays = vision(Vec::new()).sight_rays(RAYS);
        let fade = FogFade::default();
        let flat = |p: Vec2| Some((p.x, p.y));
        let clear = fog_grid(&rays, vec2(-2.0, -2.0), vec2(2.0, 2.0), &fade, 0.0);
        assert!(fog_mesh(&clear, flat).indices.is_empty());
        let dark = fog_grid(&rays, vec2(20.0, 20.0), vec2(24.0, 24.0), &fade, 0.0);
        let mesh = fog_mesh(&dark, flat);
        assert!(!mesh.indices.is_empty());
        assert!(mesh.colors.iter().all(|c| c.a > 0));
        assert!(fog_mesh(&dark, |_| None).indices.is_empty());
    }

    #[test]
    fn no_seeing_tank_darkens_everything() {
        let rays = TeamVision::default().sight_rays(RAYS);
        assert_eq!(darkness(&rays, Vec2::zeros()), DARKNESS);
    }

    /// Times the fog grid and mesh for one tank in a field of cover; run
    /// with `cargo test --release fog_mesh_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn fog_mesh_cost() {
        let cover = (0..24)
            .map(|i| {
                let angle = i as f32 * 0.7;
                let distance = 3.0 + (i % 6) as f32 * 2.0;
                GroundRect::axis_aligned(
                    vec2(angle.cos(), angle.sin()) * distance,
                    vec2(1.0 + (i % 3) as f32, 1.0),
                )
            })
            .collect();
        let mut vision = vision(cover);
        vision.viewers[0].range = 15.0;
        let reach = vision.viewers[0].range + SOFT_EDGE + 1.0;
        let frames = 60;
        let mut fade = FogFade::default();
        let start = std::time::Instant::now();
        let mut triangles = 0;
        for frame in 0..frames {
            let rays = vision.sight_rays(RAYS);
            let dt = fade.step(frame as f32 / 60.0);
            let grid = fog_grid(&rays, vec2(-reach, -reach), vec2(reach, reach), &fade, dt);
            let mesh = fog_mesh(&grid, |p| Some((p.x, p.y)));
            fade.grid = Some(grid);
            triangles += mesh.indices.len() / 3;
        }
        let each = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!(
            "fog mesh: {each:.3} ms per frame, {} triangles",
            triangles / frames
        );
    }
}
