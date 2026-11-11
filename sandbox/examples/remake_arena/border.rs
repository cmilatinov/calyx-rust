//! The arena's border: an irregular outline with no straight side, every
//! stretch of it turning: lobes push out round the orchard in the
//! northwest, the barn yard in the northeast, the middle of the east, the
//! supply depot in the southeast and the farm in the southwest, a
//! peninsula reaches north behind the north spawn and south behind the
//! south spawn, and coves bite in between them.
//! The creek comes in through a gap on the west, and its two branches leave
//! through inlets on the north and the south, where its blockers close the
//! border to tanks.
//!
//! The border is a farm's field boundary: a dry-stone wall, one mesh swept
//! along the outline and rounded round every bend, and a hedgerow round the
//! orchard's lobe. Positions are on the border's centre line, in the
//! Arena's frame.

use calyx_engine::assets::AssetRef;
use calyx_engine::component::ComponentMesh;
use calyx_engine::math::Transform;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

use super::{asset_id, blocking_shells, farmland::Creek, stable, Author};

/// What the stretch of border from a vertex to the next is built of.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The stone wall, swept along each line of stone stretches.
    Stone,
    /// Hedgerows.
    Hedge,
    /// A creek gap: the border stops at the creek's banks either side, and
    /// the creek's blockers close the gap to tanks.
    Creek,
}

impl Kind {
    /// The kit model, how long one stands along the run, and how thick
    /// the run's solid is. Stone draws no kit model: its wall is swept
    /// along the outline.
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
const OUTLINE: [(f32, f32, Kind); 59] = [
    // The west side north of the creek, then the orchard's lobe, pushed
    // out west and north so the orchard has room round its windmill (#580).
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
    // The barn yard's lobe, pushed out round the paddocks.
    (11.0, 38.5, Kind::Stone),
    (15.0, 42.0, Kind::Stone),
    (21.5, 43.0, Kind::Stone),
    (27.0, 40.0, Kind::Stone),
    (32.5, 39.5, Kind::Stone),
    (36.5, 34.5, Kind::Stone),
    (41.0, 31.0, Kind::Stone),
    (40.0, 24.5, Kind::Stone),
    // A cove by the silo, then the east's bulge.
    (36.5, 21.0, Kind::Stone),
    (34.0, 16.5, Kind::Stone),
    (36.0, 12.0, Kind::Stone),
    (40.5, 9.5, Kind::Stone),
    (42.5, 5.5, Kind::Stone),
    (41.5, -1.0, Kind::Stone),
    (37.0, -4.5, Kind::Stone),
    (33.5, -9.0, Kind::Stone),
    // A cove above the depot, and the depot's lobe pushing out south-east
    // round its yard, a bay biting into it between the shed and the fuel
    // tank and another along its south side.
    (30.0, -11.5, Kind::Stone),
    (34.5, -15.0, Kind::Stone),
    (39.5, -18.0, Kind::Stone),
    (41.0, -24.0, Kind::Stone),
    (38.5, -29.0, Kind::Stone),
    (42.0, -33.5, Kind::Stone),
    (37.5, -38.5, Kind::Stone),
    (31.0, -37.5, Kind::Stone),
    (26.5, -41.0, Kind::Stone),
    (22.5, -38.5, Kind::Creek),
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

/// The stone wall's cross-section: half its thickness, its height, and
/// the radius and segments of the bevel rounding its four long edges, the
/// kit's wall modules' (`asset_sources/models/wall_long.toml`).
const WALL_HALF: f32 = 0.6;
const WALL_TOP: f32 = 1.6;
const WALL_BEVEL: f32 = 0.12;
const BEVEL_SEGMENTS: usize = 3;
/// About how long one block of the wall is between seams, as the kit's
/// long module.
const BLOCK_LENGTH: f32 = 4.0;
/// The shortest stretch the wall sweeps from a rounded end to a bend.
const MIN_STRETCH: f32 = 1.0;
/// The most the wall turns between two stations of a rounded bend or end.
const ARC_STEP: f32 = 10.0;

/// The asset type of a mesh, as the kit's models are registered.
const MESH_TYPE: &str = "792d264b-de6f-4431-b59f-76f18fdb3bfe";

/// The wall's cross-section, from the foot of its left face (`across`
/// positive) over the top to the foot of its right: each point's offset
/// across the centre line, height, and the normal's across and up
/// components. The first [`HALF_PROFILE`] points run from the left foot to
/// the top's middle.
fn profile() -> Vec<[f32; 4]> {
    let side = WALL_HALF - WALL_BEVEL;
    let mut points = Vec::new();
    for (center, from) in [(WALL_BEVEL, -90.0), (WALL_TOP - WALL_BEVEL, 0.0)] {
        for k in 0..=BEVEL_SEGMENTS {
            let angle: f32 = from + 90.0 * k as f32 / BEVEL_SEGMENTS as f32;
            let (sin, cos) = angle.to_radians().sin_cos();
            points.push([side + WALL_BEVEL * cos, center + WALL_BEVEL * sin, cos, sin]);
        }
    }
    points.push([0.0, WALL_TOP, 0.0, 1.0]);
    let right: Vec<_> = points[..HALF_PROFILE - 1]
        .iter()
        .rev()
        .map(|&[across, up, normal_across, normal_up]| [-across, up, -normal_across, normal_up])
        .collect();
    points.extend(right);
    points
}

/// The points of [`profile`] from the left foot to the top's middle.
const HALF_PROFILE: usize = 2 * (BEVEL_SEGMENTS + 1) + 1;

/// A triangle mesh in the Arena's frame: each corner's position and normal.
#[derive(Default)]
struct Mesh {
    triangles: Vec<[(Vec3, Vec3); 3]>,
}

impl Mesh {
    /// The triangle `a`, `b`, `c`, wound to face its corners' normals;
    /// nothing when it has no area, as where a bend's inner side folds
    /// onto its mitre.
    fn triangle(&mut self, a: (Vec3, Vec3), b: (Vec3, Vec3), c: (Vec3, Vec3)) {
        let face = (b.0 - a.0).cross(&(c.0 - a.0));
        if face.norm() < 1e-6 {
            return;
        }
        if face.dot(&(a.1 + b.1 + c.1)) < 0.0 {
            self.triangles.push([a, c, b]);
        } else {
            self.triangles.push([a, b, c]);
        }
    }

    /// The faces between two stations of the same points.
    fn strip(&mut self, from: &[(Vec3, Vec3)], to: &[(Vec3, Vec3)]) {
        for j in 0..from.len() - 1 {
            self.triangle(from[j], from[j + 1], to[j + 1]);
            self.triangle(from[j], to[j + 1], to[j]);
        }
    }

    /// A stone wall along `line`, its ends rounded: the profile swept
    /// along each stretch, round the outside of each bend and mitred on
    /// its inside, so the wall is one surface from end to end.
    fn wall(&mut self, line: &[Vec2]) {
        let profile = profile();
        let lift = |at: Vec2, across: Vec2, point: [f32; 4]| {
            (
                vec3(at.x, point[1], at.y),
                vec3(across.x * point[2], point[3], across.y * point[2]),
            )
        };
        let rotate = |v: Vec2, angle: f32| {
            let (sin, cos) = angle.sin_cos();
            vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
        };
        let steps = |angle: f32| (angle.abs().to_degrees() / ARC_STEP).ceil().max(1.0) as usize;
        let along: Vec<Vec2> = line.windows(2).map(|w| (w[1] - w[0]).normalize()).collect();
        let across: Vec<Vec2> = along.iter().map(|d| vec2(-d.y, d.x)).collect();
        // Each bend's turn, positive towards `across`.
        let turn = |i: usize| {
            let (a, b) = (along[i - 1], along[i]);
            (a.x * b.y - a.y * b.x).atan2(a.dot(&b))
        };
        // How far a point `offset` across the line moves along a stretch
        // to meet its neighbour's at the bend: only on the bend's inside.
        let mitre = |turn: f32, offset: f32| {
            if offset * turn > 0.0 {
                offset.abs() * (turn.abs() * 0.5).tan()
            } else {
                0.0
            }
        };
        let last = line.len() - 1;
        // Where the point `offset` across the line meets the bend at
        // `line[j]`, when it is on the bend's inside: one position, worked
        // out once, for both stretches and the bend.
        let inner = |j: usize, offset: f32| {
            let shift = mitre(turn(j), offset);
            (shift > 0.0).then(|| line[j] + across[j - 1] * offset - along[j - 1] * shift)
        };
        for i in 0..last {
            let station = |j: usize| -> Vec<_> {
                profile
                    .iter()
                    .map(|&point| {
                        let offset = point[0];
                        let bend = (j > 0 && j < last).then(|| inner(j, offset)).flatten();
                        lift(
                            bend.unwrap_or(line[j] + across[i] * offset),
                            across[i],
                            point,
                        )
                    })
                    .collect()
            };
            // The stretch, with a seam between blocks about every
            // [`BLOCK_LENGTH`] along it, as the kit's modules meet: each
            // block's bevel turns in to the seam, where the two meet in a
            // crease the ink outlines.
            let length = (line[i + 1] - line[i]).norm();
            let blocks = (length / BLOCK_LENGTH).round().max(1.0) as usize;
            // A station at `x` along the stretch, each point set in
            // `inset` along its normal, the normal tipped `tip` towards
            // the stretch's end.
            let block = |x: f32, inset: f32, tip: f32| -> Vec<_> {
                let at = line[i] + along[i] * x;
                profile
                    .iter()
                    .map(|&[offset, up, normal_across, normal_up]| {
                        // The foot's bevel is set in level, so the seam
                        // runs down to the ground.
                        let point = if up < WALL_BEVEL {
                            [
                                offset - inset * offset.signum(),
                                up,
                                normal_across,
                                normal_up,
                            ]
                        } else {
                            [
                                offset - inset * normal_across,
                                up - inset * normal_up,
                                normal_across,
                                normal_up,
                            ]
                        };
                        let (sin, cos) = tip.sin_cos();
                        let (position, normal) = lift(at + across[i] * point[0], across[i], point);
                        let normal = normal * cos + vec3(along[i].x, 0.0, along[i].y) * sin;
                        (position, normal)
                    })
                    .collect()
            };
            let mut from = station(i);
            for seam in 1..blocks {
                let x = length * seam as f32 / blocks as f32;
                // The bevel ending the block before the seam, then the one
                // starting the block after it.
                for side in [-1.0f32, 1.0] {
                    let bevel: Vec<_> = (0..=BEVEL_SEGMENTS)
                        .map(|k| {
                            let k = if side < 0.0 { k } else { BEVEL_SEGMENTS - k };
                            let angle =
                                std::f32::consts::FRAC_PI_2 * k as f32 / BEVEL_SEGMENTS as f32;
                            block(
                                x + side * WALL_BEVEL * (1.0 - angle.sin()),
                                WALL_BEVEL * (1.0 - angle.cos()),
                                -side * angle,
                            )
                        })
                        .collect();
                    self.strip(&from, &bevel[0]);
                    for pair in bevel.windows(2) {
                        self.strip(&pair[0], &pair[1]);
                    }
                    from = bevel[BEVEL_SEGMENTS].clone();
                }
            }
            self.strip(&from, &station(i + 1));
        }
        // Round the outside of each bend; its inside points stay on the
        // mitre, where their faces fold away.
        for i in 1..last {
            let turn = turn(i);
            let at = line[i];
            let count = steps(turn);
            let stations: Vec<Vec<_>> = (0..=count)
                .map(|k| {
                    // Its first and last stations are the stretches' own,
                    // exactly, so the sweeps share their corners.
                    let side = match k {
                        0 => across[i - 1],
                        k if k == count => across[i],
                        k => rotate(across[i - 1], turn * k as f32 / count as f32),
                    };
                    profile
                        .iter()
                        .map(|&point| {
                            let place = inner(i, point[0]).unwrap_or(at + side * point[0]);
                            lift(place, side, point)
                        })
                        .collect()
                })
                .collect();
            for pair in stations.windows(2) {
                self.strip(&pair[0], &pair[1]);
            }
        }
        // Round each end: the left half of the profile turned half a turn
        // about the end, behind the first stretch and past the last.
        let half = &profile[..HALF_PROFILE];
        for (at, side, sense) in [
            (line[0], across[0], 1.0),
            (line[last], across[last - 1], -1.0),
        ] {
            let count = steps(std::f32::consts::PI);
            let stations: Vec<Vec<_>> = (0..=count)
                .map(|k| {
                    let side = match k {
                        0 => side,
                        k if k == count => -side,
                        k => rotate(side, sense * std::f32::consts::PI * k as f32 / count as f32),
                    };
                    half.iter()
                        .map(|&point| lift(at + side * point[0], side, point))
                        .collect()
                })
                .collect();
            for pair in stations.windows(2) {
                self.strip(&pair[0], &pair[1]);
            }
        }
    }

    /// Writes the mesh as an OBJ, positions and normals shared between
    /// corners that match.
    fn write(&self, path: &std::path::Path) {
        use std::collections::HashMap;
        use std::fmt::Write;
        let mut positions: HashMap<String, usize> = HashMap::new();
        let mut normals: HashMap<String, usize> = HashMap::new();
        let (mut vertex_lines, mut normal_lines, mut faces) =
            (String::new(), String::new(), String::new());
        let index =
            |table: &mut HashMap<String, usize>, lines: &mut String, prefix, key: String| {
                let next = table.len() + 1;
                *table.entry(key.clone()).or_insert_with(|| {
                    writeln!(lines, "{prefix} {key}").unwrap();
                    next
                })
            };
        for triangle in &self.triangles {
            let corners: Vec<String> = triangle
                .iter()
                .map(|(p, n)| {
                    let n = n.normalize();
                    let v = index(
                        &mut positions,
                        &mut vertex_lines,
                        "v",
                        // To a tenth of a millimetre, so the stations of
                        // neighbouring sweeps that meet share their corners.
                        format!("{:.4} {:.4} {:.4}", p.x + 0.0, p.y + 0.0, p.z + 0.0),
                    );
                    let n = index(
                        &mut normals,
                        &mut normal_lines,
                        "vn",
                        format!("{:.4} {:.4} {:.4}", n.x + 0.0, n.y + 0.0, n.z + 0.0),
                    );
                    format!("{v}//{n}")
                })
                .collect();
            writeln!(faces, "f {}", corners.join(" ")).unwrap();
        }
        std::fs::write(
            path,
            format!(
                "# Generated by remake_arena's border module from its outline; regenerate rather than edit\n{vertex_lines}{normal_lines}{faces}"
            ),
        )
        .unwrap();
    }
}

impl Author {
    /// The border, segment by segment: each an object turned along it
    /// holding one solid along its whole length, to the shell-blocking top,
    /// that stops tanks, rounds and sight, and a hedge segment its
    /// hedgerows too. A segment's solid reaches half its thickness past
    /// each bend, so neighbouring solids overlap and no tank slips between
    /// them. The stone is one mesh, `Arena Wall Border Stones`: a wall
    /// swept along each unbroken line of stone stretches, rounded round
    /// the outside of each bend and at each end, where it meets a hedgerow
    /// or stops on a creek's bank.
    pub fn border(&mut self, creek: &Creek) {
        let mut hedges = 0;
        let mut index = 0;
        let mut lines: Vec<Vec<Vec2>> = Vec::new();
        let mut line: Vec<Vec2> = Vec::new();
        let solid_runs = runs(creek, CREEK_SOLID_END);
        let piece_runs = runs(creek, CREEK_PIECE_END);
        // The last stretch of a run, to the bank, is stone.
        let kind_of = |kind| {
            if kind == Kind::Creek {
                Kind::Stone
            } else {
                kind
            }
        };
        for (run, pieces) in solid_runs.iter().zip(&piece_runs) {
            for i in 0..run.len() - 1 {
                let (a, kind, a_bend) = run[i];
                let (b, _, b_bend) = run[i + 1];
                let kind = kind_of(kind);
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
                if kind == Kind::Stone {
                    // The stone's line: from its end on a bank, the wall's
                    // rounded end reaching the pieces' end there, or from
                    // its meeting with a hedgerow, through each bend.
                    // Where the bank comes within a stub of a bend, the
                    // wall's rounded end stands on the bend instead.
                    let stub = |end: Vec2, bend: Vec2| (end - bend).norm() < MIN_STRETCH;
                    if line.is_empty() {
                        let start = pieces[i].0 + along * WALL_HALF;
                        if a_bend || !stub(start, b) {
                            line.push(if a_bend { a } else { start });
                        }
                    }
                    if b_bend && kind_of(run[i + 1].1) == Kind::Stone {
                        line.push(b);
                    } else {
                        let end = pieces[i + 1].0 - along * WALL_HALF;
                        if b_bend || line.len() < 2 || !stub(end, a) {
                            line.push(if b_bend { b } else { end });
                        }
                        lines.push(std::mem::take(&mut line));
                    }
                    continue;
                }
                // The hedgerows: from bend to bend, overlapping the next
                // stretch's at a bend between them, and starting clear of
                // the stone wall's rounded end where they meet it, spread
                // evenly, each turned a little so the row reads grown, not
                // laid.
                let stone_before = i > 0 && kind_of(run[i - 1].1) == Kind::Stone;
                let stone_after = kind_of(run[i + 1].1) == Kind::Stone;
                let piece_start = if stone_before { WALL_HALF } else { start };
                let piece_end = if stone_after {
                    (b - a).norm() - WALL_HALF
                } else {
                    end
                };
                let span = piece_end - piece_start;
                // Never so few that the row leaves gaps: a hedgerow is
                // 4.4 m long.
                let n = (span / step).round().max((span / 4.4).ceil()).max(1.0) as usize;
                let spacing = span / n as f32;
                let mid = (start + end) * 0.5;
                for k in 0..n {
                    let x = piece_start + spacing * (k as f32 + 0.5) - mid;
                    let jitter = [0.0, 3.0, -2.0][k % 3];
                    self.kit_under(
                        segment,
                        model,
                        &format!("{name} Piece {k}"),
                        vec3(x, 0.0, 0.0),
                        jitter,
                    );
                    hedges += 1;
                }
            }
        }
        assert!(hedges > 8, "the border has {hedges} hedgerows");
        assert!(line.is_empty(), "a stone line is left open");
        let mut mesh = Mesh::default();
        for line in &lines {
            mesh.wall(line);
        }
        let folder = self.assets.join("generated/border");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("stone_wall.obj");
        mesh.write(&path);
        // Its meta as the kit's models' are written, so the editor keeps it.
        let mesh_id = stable("asset/generated/border/stone_wall");
        std::fs::write(
            path.with_extension("meta"),
            format!(
                "{{
  \"main\": {{
    \"id\": \"{mesh_id}\",
    \"name\": \"generated/border/stone_wall\",
    \"display_name\": \"stone_wall\",
    \"type_uuid\": \"{MESH_TYPE}\"
  }},
  \"inner\": []
}}"
            ),
        )
        .unwrap();
        let material = asset_id(&self.assets.join(format!(
            "generated/materials/{}/walls.cxmat",
            self.palette.folder()
        )));
        let stones = self.object("Arena Wall Border Stones", self.arena, Vec3::zeros());
        self.scene.add_component(
            stones,
            ComponentMesh {
                mesh: AssetRef::from_id(mesh_id),
                material: AssetRef::from_id(material),
                cast_shadows: true,
            },
        );
    }
}

/// The border wall's inner and outer faces, as two closed rings in the
/// Arena's frame, for fog of war: the outline offset either side by half
/// the thickness of each stretch's solid, the faces of neighbouring
/// stretches meeting at each bend where the solids' faces cross. The creek
/// gaps are closed as if a stone stretch stood in them, so sight never
/// leaves the pen through them.
pub fn faces() -> (Vec<Vec2>, Vec<Vec2>) {
    let ring = polygon();
    let n = ring.len();
    // Twice the signed area, positive when the outline runs
    // counterclockwise in (x, z), where the inside lies to the left.
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let inward = area.signum();
    let face = |side: f32| -> Vec<Vec2> {
        // Stretch `i`'s face on `side` (1 inside, -1 outside): a point on
        // it and its direction.
        let line = |i: usize| {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            let along = (b - a).normalize();
            let half = OUTLINE[i].2.piece().2 * 0.5;
            (a + vec2(-along.y, along.x) * (inward * side * half), along)
        };
        (0..n)
            .map(|i| {
                let (p, d) = line((i + n - 1) % n);
                let (q, e) = line(i);
                let denom = d.x * e.y - d.y * e.x;
                if denom.abs() < 1e-6 {
                    return q;
                }
                let w = q - p;
                p + d * ((w.x * e.y - w.y * e.x) / denom)
            })
            .collect()
    };
    (face(1.0), face(-1.0))
}
