//! The arena's border: an irregular outline with no straight side, every
//! stretch of it turning: lobes push out round the orchard in the
//! northwest, the barn yard in the northeast, the middle of the east and
//! the farm in the southwest, a peninsula reaches north behind the north
//! spawn and south behind the south spawn, and coves bite in between them.
//! The creek comes in through a gap on the west, and its two branches leave
//! through inlets on the north and the south, where its blockers close the
//! border to tanks.
//!
//! The border is a farm's field boundary: a dry-stone wall, its modules
//! stretched along each stretch, with a boulder set at every bend, and a
//! hedgerow round the orchard's lobe. Positions are on the border's centre
//! line, in the Arena's frame.

use calyx_engine::math::Transform;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2};

use super::{blocking_shells, farmland::Creek, Author};

/// What the stretch of border from a vertex to the next is built of.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The kit's stone wall modules, stretched to fit each segment, with a
    /// boulder at each bend.
    Stone,
    /// Hedgerows.
    Hedge,
    /// A creek gap: the border stops at the creek's banks either side, and
    /// the creek's blockers close the gap to tanks.
    Creek,
}

impl Kind {
    /// The kit model, how long one stands along the run, and how thick
    /// the run's solid is.
    fn piece(self) -> (&'static str, f32, f32) {
        match self {
            Kind::Stone | Kind::Creek => ("wall_long", 4.0, 1.2),
            Kind::Hedge => ("hedgerow", 4.0, 1.3),
        }
    }
}

/// The border's outline, going round the pen clockwise seen from above
/// (west, north, east, south), each vertex with what the stretch to the
/// next one is built of. The west gap's last vertex closes the ring back to
/// the first.
const OUTLINE: [(f32, f32, Kind); 56] = [
    // The west side north of the creek, then the orchard's lobe, pushed
    // out west and north so the orchard has room: a clearing in the lobe
    // and wide lanes between its rows (#580).
    (-33.5, 8.5, Kind::Stone),
    (-29.0, 8.5, Kind::Stone),
    (-30.0, 12.5, Kind::Hedge),
    (-35.0, 14.5, Kind::Hedge),
    (-40.0, 19.0, Kind::Hedge),
    (-40.5, 25.0, Kind::Hedge),
    (-37.5, 30.5, Kind::Hedge),
    (-31.5, 31.5, Kind::Hedge),
    (-28.5, 35.5, Kind::Hedge),
    (-21.0, 35.5, Kind::Stone),
    // The peninsula behind the north spawn, and the north branch's inlet.
    (-17.0, 38.5, Kind::Stone),
    (-10.5, 41.0, Kind::Stone),
    (-5.5, 38.0, Kind::Stone),
    (-4.5, 33.5, Kind::Creek),
    (10.0, 33.0, Kind::Stone),
    // The barn yard's lobe.
    (11.0, 37.5, Kind::Stone),
    (14.5, 39.0, Kind::Stone),
    (20.5, 37.0, Kind::Stone),
    (24.0, 33.5, Kind::Stone),
    (28.0, 32.5, Kind::Stone),
    (30.5, 27.5, Kind::Stone),
    (34.5, 25.0, Kind::Stone),
    // A cove by the silo, then the east's bulge.
    (33.0, 19.5, Kind::Stone),
    (29.5, 16.5, Kind::Stone),
    (31.0, 12.0, Kind::Stone),
    (37.5, 10.0, Kind::Stone),
    (42.5, 5.5, Kind::Stone),
    (41.5, -1.0, Kind::Stone),
    (37.0, -4.5, Kind::Stone),
    (33.5, -9.0, Kind::Stone),
    // A cove above the depot, and the depot's corner.
    (30.0, -11.5, Kind::Stone),
    (33.5, -15.5, Kind::Stone),
    (34.5, -21.5, Kind::Stone),
    (32.5, -27.0, Kind::Stone),
    (35.0, -31.5, Kind::Stone),
    (29.5, -36.0, Kind::Stone),
    (24.0, -34.5, Kind::Stone),
    (22.0, -31.0, Kind::Creek),
    // The south branch's inlet, the peninsula behind the south spawn.
    (10.5, -30.0, Kind::Stone),
    (7.0, -28.0, Kind::Stone),
    (3.5, -31.5, Kind::Stone),
    (1.5, -36.5, Kind::Stone),
    (-4.0, -39.0, Kind::Stone),
    (-10.0, -36.0, Kind::Stone),
    (-13.5, -33.0, Kind::Stone),
    // The farm's lobe.
    (-19.0, -35.5, Kind::Stone),
    (-25.5, -40.0, Kind::Stone),
    (-33.5, -38.0, Kind::Stone),
    (-37.0, -32.5, Kind::Stone),
    (-34.5, -27.5, Kind::Stone),
    (-38.5, -22.0, Kind::Stone),
    (-36.5, -16.5, Kind::Stone),
    // The west side, a cove south of the creek.
    (-30.5, -13.5, Kind::Stone),
    (-29.0, -8.5, Kind::Stone),
    (-31.5, -6.0, Kind::Stone),
    (-34.0, -4.0, Kind::Creek),
];

/// How far past the creek's shore a border run ends: its solid reaches
/// into the creek's blockers, which stop tanks from there; its pieces
/// stop on the bank, short of the carved ground.
const CREEK_SOLID_END: f32 = 0.2;
const CREEK_PIECE_END: f32 = 0.8;

/// The outline's vertices, as a closed ring.
pub fn polygon() -> Vec<Vec2> {
    OUTLINE.iter().map(|&(x, z, _)| vec2(x, z)).collect()
}

/// Whether `p` lies inside the border's outline (its centre line).
pub fn inside(p: Vec2) -> bool {
    let ring = polygon();
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// How far `p` lies from the border's centre line, whichever side.
pub fn distance(p: Vec2) -> f32 {
    let ring = polygon();
    (0..ring.len())
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let ab = b - a;
            let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
            (p - (a + ab * f)).norm()
        })
        .fold(f32::MAX, f32::min)
}

/// How far `p` lies outside the border's centre line: negative inside.
pub fn beyond(p: Vec2) -> f32 {
    if inside(p) {
        -distance(p)
    } else {
        distance(p)
    }
}

/// Where the gap edge from `a` towards `b` first comes within `beyond` of
/// the creek's shore.
fn to_shore(creek: &Creek, a: Vec2, b: Vec2, beyond: f32) -> Vec2 {
    let steps = ((b - a).norm() / 0.05).ceil() as usize;
    (0..=steps)
        .map(|i| a + (b - a) * (i as f32 / steps as f32))
        .find(|&p| creek.beyond(p) < beyond)
        .expect("a creek gap crosses the creek")
}

/// The border's runs, between creek gaps: each a list of `(point, kind)`,
/// the kind building the stretch to the next point, its first and last
/// points on a creek bank where the solid stops `beyond` the shore.
fn runs(creek: &Creek, beyond: f32) -> Vec<Vec<(Vec2, Kind, bool)>> {
    let n = OUTLINE.len();
    let gap = OUTLINE
        .iter()
        .position(|&(_, _, kind)| kind == Kind::Creek)
        .unwrap();
    let mut runs = Vec::new();
    let mut run: Vec<(Vec2, Kind, bool)> = Vec::new();
    // Start just after a gap, so every run lies between two of them.
    for k in 1..=n {
        let i = (gap + k) % n;
        let (x, z, kind) = OUTLINE[i];
        let (prev_x, prev_z, _) = OUTLINE[(i + n - 1) % n];
        let at = vec2(x, z);
        if run.is_empty() {
            // The run starts where the gap's far side meets the bank.
            let start = to_shore(creek, at, vec2(prev_x, prev_z), beyond);
            run.push((start, Kind::Stone, false));
        }
        if kind == Kind::Creek {
            let (next_x, next_z, _) = OUTLINE[(i + 1) % n];
            run.push((at, kind, true));
            let end = to_shore(creek, at, vec2(next_x, next_z), beyond);
            run.push((end, kind, false));
            runs.push(std::mem::take(&mut run));
        } else {
            run.push((at, kind, true));
        }
    }
    runs
}

/// The border's bounding rectangle, outer faces included: its least and
/// greatest corners.
pub fn bounds() -> (Vec2, Vec2) {
    let mut low = vec2(f32::MAX, f32::MAX);
    let mut high = vec2(f32::MIN, f32::MIN);
    for point in polygon() {
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
    /// each bend, so neighbouring solids overlap and no tank slips between
    /// them; a boulder set at each bend of a stone run hides the joint.
    pub fn border(&mut self, creek: &Creek) {
        let mut count = 0;
        let mut index = 0;
        let solid_runs = runs(creek, CREEK_SOLID_END);
        let piece_runs = runs(creek, CREEK_PIECE_END);
        for (run, pieces) in solid_runs.iter().zip(&piece_runs) {
            for i in 0..run.len() - 1 {
                let (a, kind, a_bend) = run[i];
                let (b, _, b_bend) = run[i + 1];
                // The last stretch of a run, to the bank, is stone.
                let kind = if kind == Kind::Creek {
                    Kind::Stone
                } else {
                    kind
                };
                let (model, step, thickness) = kind.piece();
                assert!(
                    (b - a).norm() > 1.0,
                    "border stretch {index} from {a:?} to {b:?} is too short"
                );
                let along = (b - a).normalize();
                let yaw = (-along.y).atan2(along.x).to_degrees();
                // A bend gets the overlap; a creek end stops at the bank.
                let start = if a_bend { -thickness * 0.5 } else { 0.0 };
                let end = (b - a).norm() + if b_bend { thickness * 0.5 } else { 0.0 };
                let name = match kind {
                    Kind::Hedge => format!("Arena Cover Border Hedgerow {index}"),
                    _ => format!("Arena Wall Border {index}"),
                };
                index += 1;
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
                // The pieces: from bend to bend, or to the bank at a creek
                // end, spread evenly.
                let piece_start = if a_bend {
                    start
                } else {
                    (pieces[i].0 - a).dot(&along)
                };
                let piece_end = if b_bend {
                    end
                } else {
                    (pieces[i + 1].0 - a).dot(&along)
                };
                let span = piece_end - piece_start;
                let n = (span / step).round().max(1.0) as usize;
                let spacing = span / n as f32;
                let mid = (start + end) * 0.5;
                for k in 0..n {
                    let x = piece_start + spacing * (k as f32 + 0.5) - mid;
                    // Stone modules stretch to meet; hedges keep their size,
                    // turned a little so the row reads grown, not laid.
                    let jitter = match kind {
                        Kind::Hedge => [0.0, 3.0, -2.0][k % 3],
                        _ => 0.0,
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
                // A boulder at the segment's far bend, set into the wall's
                // joint, turned and sized by the bend so no two match. It
                // stands within the overlapping solids, so it needs none.
                if b_bend && kind == Kind::Stone {
                    let model = ["rock_medium", "rock_large", "rock_medium"][index % 3];
                    let rock = self.kit(
                        model,
                        &format!("Arena Prop Border Bend {index}"),
                        vec3(b.x, -0.08, b.y),
                        (index * 67 % 360) as f32,
                    );
                    let mut pose = self.scene.transform(rock);
                    pose.scale =
                        vec3(1.0, 1.0, 1.0) * if model == "rock_large" { 0.95 } else { 1.2 };
                    self.scene.set_transform(rock, &pose.matrix());
                }
            }
        }
        assert!(count > 60, "the border has {count} pieces");
    }
}
