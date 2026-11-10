//! The arena's border: not one rectangle but a closed outline of straight
//! runs, jogging out round the corner landmarks and cut at 35 to 45 degrees
//! like the interior walls, and built of what suits each quadrant: the
//! farm's stone walls in the southwest and along the south and north,
//! concrete barriers round the supply depot, hay bales round the barn yard
//! and hedgerows along the orchard. The creek leaves through a gap on the
//! west and the east, where its blockers close the border to tanks.
//!
//! Positions are on the border's centre line, in the Arena's frame.

use calyx_engine::math::Transform;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2};

use super::{blocking_shells, farmland::Creek, Author};

/// What a run of the border is built of.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The kit's stone wall modules, stretched to fit each segment.
    Stone,
    /// Concrete barriers set end to end.
    Barriers,
    /// Round hay bales in a row.
    Bales,
    /// Hedgerows.
    Hedge,
}

impl Kind {
    /// The kit model, how long one stands along the run, and how thick
    /// the run's solid is.
    fn piece(self) -> (&'static str, f32, f32) {
        match self {
            Kind::Stone => ("wall_long", 4.0, 1.2),
            Kind::Barriers => ("barrier_plain", 2.5, 1.12),
            Kind::Bales => ("hay_bale", 1.25, 1.24),
            Kind::Hedge => ("hedgerow", 4.0, 1.3),
        }
    }

    /// The name of a segment's object: stone walls are `Arena Wall`s, on
    /// the terrain's outline layer; the rest are props, `Arena Cover
    /// Border`, which block sight as the walls do.
    fn object_name(self, run: &str, i: usize) -> String {
        match self {
            Kind::Stone => format!("Arena Wall Border {run} {i}"),
            Kind::Barriers => format!("Arena Cover Border {run} Barriers {i}"),
            Kind::Bales => format!("Arena Cover Border {run} Hay Bales {i}"),
            Kind::Hedge => format!("Arena Cover Border {run} Hedgerow {i}"),
        }
    }
}

/// Where a run starts or ends: a fixed point, or the creek's crossing of
/// the line x = `x`, on its south or north bank.
#[derive(Clone, Copy)]
enum End {
    At(f32, f32),
    CreekSouth(f32),
    CreekNorth(f32),
}

/// The border's runs, `(name, kind, points)`, going round the pen
/// clockwise seen from above (west, north, east, south): the orchard's
/// hedgerows from the west creek gap, cut across the northwest corner; the
/// north's stone wall; the barn yard's hay bales, stepping out round the
/// platform's corner; the depot's barriers, from the east creek gap
/// stepping out east round the depot; and the farm's stone walls along
/// the south and round the water tower, back to the west creek gap. Every
/// angled segment runs at 35 to 45 degrees to the axes.
const RUNS: [(&str, Kind, &[End]); 5] = [
    (
        "Orchard",
        Kind::Hedge,
        &[
            End::CreekNorth(-29.0),
            End::At(-29.0, 26.0),
            End::At(-24.0, 31.0),
            End::At(-12.0, 31.0),
        ],
    ),
    (
        "North",
        Kind::Stone,
        &[End::At(-12.0, 31.0), End::At(19.0, 31.0)],
    ),
    (
        "Barn",
        Kind::Bales,
        &[
            End::At(19.0, 31.0),
            End::At(22.0, 34.0),
            End::At(28.0, 34.0),
            End::At(31.5, 30.0),
            End::At(31.5, 16.0),
            End::At(29.0, 13.0),
            End::CreekNorth(29.0),
        ],
    ),
    (
        "Depot",
        Kind::Barriers,
        &[
            End::CreekSouth(29.0),
            End::At(29.0, -11.0),
            End::At(33.0, -15.0),
            End::At(33.0, -27.5),
            End::At(30.0, -31.0),
            End::At(11.0, -31.0),
        ],
    ),
    (
        "Farm",
        Kind::Stone,
        &[
            End::At(11.0, -31.0),
            End::At(-15.5, -31.0),
            End::At(-18.0, -34.0),
            End::At(-27.5, -34.0),
            End::At(-33.0, -27.5),
            End::At(-33.0, -22.0),
            End::At(-29.0, -18.0),
            End::CreekSouth(-29.0),
        ],
    ),
];

/// How far past the creek's shore a border run ends: its solid reaches
/// into the creek's blockers, which stop tanks from there; its pieces
/// stop on the bank, short of the carved ground.
const CREEK_SOLID_END: f32 = 0.2;
const CREEK_PIECE_END: f32 = 0.8;

/// Where the creek's bank, `beyond` its shore, crosses x = `x`: the least
/// and greatest z, south and north of the water.
fn creek_bank(creek: &Creek, x: f32, beyond: f32) -> (f32, f32) {
    let inside: Vec<f32> = (-400..400)
        .map(|i| i as f32 * 0.05)
        .filter(|&z| creek.beyond(vec2(x, z)) < beyond)
        .collect();
    (
        inside.iter().copied().fold(f32::MAX, f32::min),
        inside.iter().copied().fold(f32::MIN, f32::max),
    )
}

/// The border's outline as closed rings of segments: each run's points on
/// the border's centre line, a creek end resolved where its solid stops.
pub fn outline(creek: &Creek) -> Vec<Vec<Vec2>> {
    RUNS.iter()
        .map(|&(_, _, ends)| {
            ends.iter()
                .map(|&end| resolve(creek, end, CREEK_SOLID_END))
                .collect()
        })
        .collect()
}

fn resolve(creek: &Creek, end: End, beyond: f32) -> Vec2 {
    match end {
        End::At(x, z) => vec2(x, z),
        End::CreekSouth(x) => vec2(x, creek_bank(creek, x, beyond).0),
        End::CreekNorth(x) => vec2(x, creek_bank(creek, x, beyond).1),
    }
}

/// The border's bounding rectangle, outer faces included: its least and
/// greatest corners.
pub fn bounds(creek: &Creek) -> (Vec2, Vec2) {
    let mut low = vec2(f32::MAX, f32::MAX);
    let mut high = vec2(f32::MIN, f32::MIN);
    for point in outline(creek).into_iter().flatten() {
        low = low.inf(&point);
        high = high.sup(&point);
    }
    (low - vec2(0.7, 0.7), high + vec2(0.7, 0.7))
}

impl Author {
    /// The border, segment by segment: each an object turned along it,
    /// holding the run's pieces spread along the segment and one solid
    /// along its whole length, to the shell-blocking top, that stops tanks,
    /// rounds and sight. A segment's solid reaches half its thickness past
    /// each corner, so neighbouring solids overlap and no tank slips
    /// between them.
    pub fn border(&mut self, creek: &Creek) {
        let mut count = 0;
        for &(run, kind, ends) in &RUNS {
            let (model, step, thickness) = kind.piece();
            let solid: Vec<Vec2> = ends
                .iter()
                .map(|&end| resolve(creek, end, CREEK_SOLID_END))
                .collect();
            let pieces: Vec<Vec2> = ends
                .iter()
                .map(|&end| resolve(creek, end, CREEK_PIECE_END))
                .collect();
            let last = ends.len() - 2;
            for i in 0..=last {
                let (a, b) = (solid[i], solid[i + 1]);
                let along = (b - a).normalize();
                let yaw = (-along.y).atan2(along.x).to_degrees();
                // A corner gets the overlap; a creek end stops at the bank.
                let is_corner = |j: usize| matches!(ends[j], End::At(..));
                let start = if is_corner(i) { -thickness * 0.5 } else { 0.0 };
                let end = (b - a).norm()
                    + if is_corner(i + 1) {
                        thickness * 0.5
                    } else {
                        0.0
                    };
                let name = kind.object_name(run, i);
                let origin = a + along * ((start + end) * 0.5);
                let position = vec3(origin.x, 0.0, origin.y);
                let segment = self.object(&name, self.arena, position);
                self.scene.set_transform(
                    segment,
                    &Transform::from_components(
                        position,
                        UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                        vec3(1.0, 1.0, 1.0),
                    )
                    .matrix(),
                );
                let length = end - start;
                let (center, size) =
                    blocking_shells(vec3(0.0, 0.8, 0.0), vec3(length, 1.6, thickness));
                self.collider(segment, &format!("{name} collision"), center, size);
                // The pieces: from where the solid starts to the bank at a
                // creek end, spread evenly.
                let piece_start = if is_corner(i) {
                    start
                } else {
                    (pieces[i] - a).dot(&along)
                };
                let piece_end = if is_corner(i + 1) {
                    end
                } else {
                    (pieces[i + 1] - a).dot(&along)
                };
                let span = piece_end - piece_start;
                let n = (span / step).round().max(1.0) as usize;
                let spacing = span / n as f32;
                let mid = (start + end) * 0.5;
                for k in 0..n {
                    let x = piece_start + spacing * (k as f32 + 0.5) - mid;
                    // Stone modules stretch to meet; the props keep their
                    // size, turned a little at random so the row reads
                    // laid by hand.
                    let jitter = match kind {
                        Kind::Stone => 0.0,
                        Kind::Barriers => [0.0, 2.0, -1.5, 1.0][k % 4],
                        Kind::Bales => [0.0, 23.0, -31.0, 47.0, 12.0][(k + i) % 5],
                        Kind::Hedge => [0.0, 3.0, -2.0][k % 3],
                    };
                    let piece = self.kit_under(
                        segment,
                        model,
                        &format!("{name} Piece {k}"),
                        vec3(x, 0.0, 0.0),
                        jitter,
                    );
                    if kind == Kind::Stone {
                        let mut pose = self.scene.transform(piece);
                        pose.scale = vec3(spacing / step, 1.0, 1.0);
                        self.scene.set_transform(piece, &pose.matrix());
                    }
                    count += 1;
                }
            }
        }
        assert!(count > 40, "the border has {count} pieces");
    }
}
