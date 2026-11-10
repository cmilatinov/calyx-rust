//! The north-west corner, the orchard (#580): rows of fruit trees at the
//! layout's 37 degrees, filling the orchard's footprint and the border's
//! lobe round it, a windmill standing over them in its footprint, its cart
//! track running down an alley towards the north spawn, a clearing of open
//! ground in its yard, and stacks of picked fruit's crates in the lanes. Past
//! the border's hedgerow the rows run on, so the corner reads as the edge of
//! a larger orchard.
//!
//! The trees stand in rows `TREE_SPACING` apart along them, close enough
//! that a tank cannot slip between two neighbours, the rows `ROW_GAP` apart,
//! so the lanes between them are wider than a tank. Every fifth tree in a
//! row is left out, staggered from row to row, so tanks cross the rows
//! there and no lane is closed off; the rows break sight into short lanes.
//!
//! Positions are in the Arena's frame, where the north spawn is at
//! [`layout::NORTH_SPAWN`].

use std::sync::OnceLock;

use calyx_engine::component::{ColliderShape, ComponentCollider};
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::terrain::ComponentDriveSurface;

use super::layout::{self, beyond_rect};
use super::{border, farmland::Creek, Author};

/// The windmill: its tower's centre, in the layout's `Windmill` footprint,
/// and its yaw, which turns its door and sails (its local +Z) to face
/// south-south-east, towards the gameplay camera and down its track.
pub const WINDMILL: Vec2 = Vec2::new(-24.5, 27.0);
const WINDMILL_YAW: f32 = 150.0;
/// The windmill's solid: a square about its plinth, rising to the
/// shell-blocking top; its sails and cap stand far above it.
const WINDMILL_SOLID: f32 = 3.9;

/// The rows' angle, as the layout's other lines, and their spacing: along
/// a row a tank cannot pass between neighbouring trunks; across, the lanes
/// leave it 3.3 m.
const ROW_YAW: f32 = 37.0;
const TREE_SPACING: f32 = 2.7;
const ROW_GAP: f32 = 4.0;
/// Where the rows' grid sits, along and across them, from the windmill.
const PHASE: (f32, f32) = (1.125, 0.75);

/// The windmill's cart track: from its door down the alley between two
/// rows, towards the north spawn.
const TRACK_LENGTH: f32 = 13.0;
const TRACK_CLEAR: f32 = 1.6;
/// The clearing: open ground in the windmill's yard, beside its track, kept
/// free of trees and cover for whatever the game puts there.
pub const CLEARING: Vec2 = Vec2::new(-19.3, 23.1);
pub const CLEARING_RADIUS: f32 = 3.0;

/// Ground the rows keep clear of, `(x, z, radius)`: the layout's cover in
/// the corner, the orchard's hedgerow and wall, its boulder and the north
/// spawn's flag, each with room for a tank. The rows keep clear of the
/// crates' stacks and the windmill's yard as well, and [`Author::orchard`]
/// checks every tree against the arena's solids.
const KEEP_CLEAR: [(f32, f32, f32); 7] = [
    // The orchard's hedgerow, end to end.
    (-17.0, 16.1, 2.4),
    (-15.5, 17.5, 2.4),
    (-14.0, 18.9, 2.4),
    // The orchard's wall, along its run.
    (-17.5, 9.0, 2.2),
    (-20.0, 6.5, 2.2),
    (-22.5, 6.5, 2.6),
    // The north spawn's flag.
    (-15.5, 20.5, 1.6),
];

/// Stacks of crates of picked fruit standing in the lanes, `(name, x, z,
/// yaw)`: a large crate with a framed one on it and one at its foot, each
/// its own object. They stop rounds and sight for now, but are dressing,
/// not the corner's essential cover: crates are to become loose objects
/// tanks shove and shots knock over, so they stand in open lanes, away from
/// the north spawn's exits, with room round them to scatter, and the rows'
/// crossings never depend on them.
const CRATES: [(&str, f32, f32, f32); 2] = [
    ("Lobe", -31.5, 22.0, ROW_YAW + 8.0),
    ("North", -17.2, 30.0, ROW_YAW - 6.0),
];

/// Where the orchard ends outside the border: this far past it.
const OUTSIDE_REACH: f32 = 18.0;

/// The rows' axes: along a row, and across the rows.
fn axes() -> (Vec2, Vec2) {
    let (sin, cos) = ROW_YAW.to_radians().sin_cos();
    (vec2(cos, -sin), vec2(sin, cos))
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
    (p - (a + ab * f)).norm()
}

/// The windmill's track, from its door to its end.
fn track() -> (Vec2, Vec2) {
    let (along, _) = axes();
    (WINDMILL, WINDMILL + along * TRACK_LENGTH)
}

/// The windmill's yard: `(model, name, where, yaw)`, sacks of flour by its
/// door and round bales of hay against its side.
fn yard() -> [(&'static str, &'static str, Vec2, f32); 3] {
    let (sin, cos) = WINDMILL_YAW.to_radians().sin_cos();
    // A point in the windmill's frame, its door towards +Z.
    let local = |x: f32, z: f32| WINDMILL + vec2(x * cos + z * sin, -x * sin + z * cos);
    [
        (
            "sandbag_stack",
            "Sacks",
            local(-2.1, 2.6),
            WINDMILL_YAW + 20.0,
        ),
        (
            "hay_bale",
            "Hay Bale 0",
            local(3.2, -0.6),
            WINDMILL_YAW + 90.0,
        ),
        (
            "hay_bale",
            "Hay Bale 1",
            local(3.0, 0.65),
            WINDMILL_YAW + 90.0,
        ),
    ]
}

/// A tree of the orchard: where it stands, whether inside the border, and
/// its scale and yaw.
#[derive(Clone, Copy)]
pub struct Tree {
    pub at: Vec2,
    pub inside: bool,
    scale: f32,
    yaw: f32,
}

/// A hash of the grid cell `(i, j)`, in 0..1.
fn hash(i: i32, j: i32, seed: i32) -> f32 {
    let h =
        (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663) ^ seed.wrapping_mul(83_492_791))
            .wrapping_mul(0x2c1b_3c6d)
            .rem_euclid(10_007);
    h as f32 / 10_007.0
}

/// The orchard's trees, inside the border and out: the rows' grid, kept
/// to ground in the corner clear of everything else.
pub fn trees(creek: &Creek) -> &'static [Tree] {
    static TREES: OnceLock<Vec<Tree>> = OnceLock::new();
    TREES.get_or_init(|| {
        let (along, across) = axes();
        let lanes: Vec<_> = layout::LANES
            .iter()
            .map(|lane| layout::lane_curve(lane))
            .collect();
        let (track_start, track_end) = track();
        let mut trees = Vec::new();
        for j in -14i32..=14 {
            for i in -18..=18 {
                let at = WINDMILL
                    + along * (i as f32 * TREE_SPACING + PHASE.0)
                    + across * (j as f32 * ROW_GAP + PHASE.1);
                let beyond = border::beyond(at);
                let clear = creek.beyond(at) > 3.0
                    && (at - WINDMILL).norm() > 4.0
                    && segment_distance(at, track_start, track_end) > TRACK_CLEAR
                    && (at - CLEARING).norm() > CLEARING_RADIUS + 1.2
                    && KEEP_CLEAR
                        .iter()
                        .all(|&(x, z, r)| (at - vec2(x, z)).norm() > r)
                    && CRATES
                        .iter()
                        .all(|&(_, x, z, _)| (at - vec2(x, z)).norm() > 3.0)
                    && yard().iter().all(|&(_, _, p, _)| (at - p).norm() > 2.2);
                let inside = beyond < -1.5
                    && at.x < -13.5
                    && at.y > 9.5
                    && (at - layout::NORTH_SPAWN).norm() > 6.5
                    && layout::lane_distance(&lanes, at) > layout::LANE_HALF_WIDTH + 1.7
                    && layout::BRUSH_PATCHES
                        .iter()
                        .all(|patch| patch.beyond(at) > 1.8)
                    // The gaps tanks cross the rows by, staggered.
                    && (i + 2 * j).rem_euclid(5) != 0;
                // Outside, the rows run on round the corner's lobe and
                // behind the north spawn, clear of the fields.
                let outside = beyond > 3.5
                    && beyond < OUTSIDE_REACH
                    && at.x < -12.0
                    && at.y > 13.0
                    && super::farmland::farm_ground(at.x.floor() as i32, at.y.floor() as i32)
                        .is_none();
                if clear && (inside || outside) {
                    trees.push(Tree {
                        at,
                        inside,
                        scale: 0.88 + 0.24 * hash(i, j, 1),
                        yaw: 360.0 * hash(i, j, 2),
                    });
                }
            }
        }
        trees
    })
}

/// The orchard's paint on the ground at `p`: `(dark, dirt)`, the weights
/// of darker, uncut grass along the tree rows, under the trees, and of
/// the windmill track's worn dirt. A strip stays where the remake leaves a
/// tree out to open a pocket, as a gap in the row.
pub fn ground(p: Vec2, creek: &Creek) -> (f32, f32) {
    let smoothstep = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Each tree's strip of its row, reaching half the spacing either way.
    let (along, across) = axes();
    let mut dark: f32 = 0.0;
    for tree in trees(creek) {
        let d = p - tree.at;
        if d.x.abs() > 3.0 || d.y.abs() > 3.0 {
            continue;
        }
        let (u, v) = (d.dot(&along).abs(), d.dot(&across).abs());
        let strip = (u - TREE_SPACING * 0.5 - 0.1).max(v - 1.1);
        dark = dark.max(smoothstep(0.4, -0.2, strip));
    }
    // The track: a worn strip from the windmill's door, ruts and all.
    let (start, end) = track();
    let track = segment_distance(p, start, end);
    let wobble = (p.x * 1.7 + p.y * 0.9).sin() * 0.15;
    let dirt =
        smoothstep(1.3, 0.8, track + wobble) * smoothstep(-1.0, 1.5, (p - start).norm() - 1.5);
    (dark, dirt)
}

/// Which of `trees` to keep so the orchard closes off no ground a tank fits
/// on: on a 0.25 m grid over the corner, a tank's centre fits where it is
/// more than 1.2 m from every solid and trunk, as the arena's reachability
/// test measures it, and every such cell must be one drive from the north
/// spawn or the grid's edge. While some are not, the trunk nearest the first
/// is left out. `solids` are the arena's other solids, as
/// [`Author::solids`] gives them.
fn open_pockets(solids: &[(String, Vec2, Vec2, f32)], trees: &[Tree]) -> Vec<bool> {
    const STEP: f32 = 0.25;
    const CLEARANCE: f32 = 1.2;
    /// The trunk's solid at its largest scale.
    const TRUNK: f32 = 0.56;
    let (low, high) = (vec2(-44.0, 2.0), vec2(-2.0, 46.0));
    let columns = ((high.x - low.x) / STEP) as usize;
    let rows = ((high.y - low.y) / STEP) as usize;
    let cell = |c: usize, r: usize| low + vec2((c as f32 + 0.5) * STEP, (r as f32 + 0.5) * STEP);
    // The distance from `p` to a box `size` across, turned `yaw` degrees.
    let distance = |p: Vec2, center: Vec2, size: Vec2, yaw: f32| {
        let (sin, cos) = yaw.to_radians().sin_cos();
        let d = p - center;
        let local = vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
        vec2(
            (local.x.abs() - size.x * 0.5).max(0.0),
            (local.y.abs() - size.y * 0.5).max(0.0),
        )
        .norm()
    };
    let near = |center: Vec2, size: Vec2| {
        let reach = size.norm() * 0.5 + CLEARANCE + 1.0;
        center.x + reach > low.x
            && center.x - reach < high.x
            && center.y + reach > low.y
            && center.y - reach < high.y
    };
    let solids: Vec<_> = solids
        .iter()
        .filter(|(_, center, size, _)| near(*center, *size))
        .collect();
    let fixed: Vec<bool> = (0..rows * columns)
        .map(|i| {
            let p = cell(i % columns, i / columns);
            solids
                .iter()
                .all(|(_, center, size, yaw)| distance(p, *center, *size, *yaw) > CLEARANCE)
        })
        .collect();
    let mut kept = vec![true; trees.len()];
    loop {
        let free: Vec<bool> = (0..rows * columns)
            .map(|i| {
                let p = cell(i % columns, i / columns);
                fixed[i]
                    && trees.iter().zip(&kept).all(|(tree, kept)| {
                        !*kept
                            || !tree.inside
                            || distance(p, tree.at, vec2(TRUNK, TRUNK), 0.0) > CLEARANCE
                    })
            })
            .collect();
        // Flood from the north spawn and the grid's edge.
        let mut reached = vec![false; free.len()];
        let mut queue = std::collections::VecDeque::new();
        let spawn = layout::NORTH_SPAWN - low;
        let start = (spawn.y / STEP) as usize * columns + (spawn.x / STEP) as usize;
        let edge = (0..rows * columns).filter(|&i| {
            let (c, r) = (i % columns, i / columns);
            c == 0 || r == 0 || c == columns - 1 || r == rows - 1
        });
        for i in std::iter::once(start).chain(edge) {
            if free[i] && !reached[i] {
                reached[i] = true;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let (c, r) = (i % columns, i / columns);
            let mut visit = |j: usize| {
                if free[j] && !reached[j] {
                    reached[j] = true;
                    queue.push_back(j);
                }
            };
            if c > 0 {
                visit(i - 1);
            }
            if c + 1 < columns {
                visit(i + 1);
            }
            if r > 0 {
                visit(i - columns);
            }
            if r + 1 < rows {
                visit(i + columns);
            }
        }
        let Some(pocket) = (0..free.len()).find(|&i| free[i] && !reached[i]) else {
            return kept;
        };
        let p = cell(pocket % columns, pocket / columns);
        let nearest = (0..trees.len())
            .filter(|&i| kept[i] && trees[i].inside)
            .min_by(|&a, &b| {
                (trees[a].at - p)
                    .norm()
                    .total_cmp(&(trees[b].at - p).norm())
            })
            .unwrap_or_else(|| panic!("ground at {p:?} is closed off by no orchard tree"));
        kept[nearest] = false;
    }
}

impl Author {
    /// The orchard's windmill, trees, fruit crates and hay, and its rows
    /// outside the border. Run after the layout and the farmland, whose
    /// solids it checks its trees against.
    pub fn orchard(&mut self, creek: &Creek) {
        self.windmill();
        for &(name, x, z, yaw) in &CRATES {
            let name = format!("Arena Cover Orchard Crate {name}");
            let (sin, cos) = yaw.to_radians().sin_cos();
            let local = |a: f32, b: f32| vec2(x + a * cos + b * sin, z - a * sin + b * cos);
            self.prop_at("crate_large", &name, vec3(x, 0.0, z), yaw);
            self.prop_at(
                "crate_framed",
                &format!("{name} Top"),
                vec3(x + 0.1, 1.65, z),
                yaw + 12.0,
            );
            let foot = local(1.45, 0.25);
            self.prop_at(
                "crate_framed",
                &format!("{name} Foot"),
                vec3(foot.x, 0.0, foot.y),
                yaw - 9.0,
            );
        }
        for (model, name, at, yaw) in yard() {
            self.prop(
                model,
                &format!("Arena Cover Windmill {name}"),
                at.x,
                at.y,
                yaw,
            );
        }
        let solids = self.solids();
        let kept = open_pockets(&solids, trees(creek));
        for (i, tree) in trees(creek).iter().enumerate() {
            if !kept[i] {
                continue;
            }
            let name = format!("Arena Prop Orchard Row Tree {i}");
            let root = self.kit(
                "orchard_tree",
                &name,
                vec3(tree.at.x, 0.0, tree.at.y),
                tree.yaw,
            );
            let mut pose = self.scene.transform(root);
            pose.scale = Vec3::repeat(tree.scale);
            self.scene.set_transform(root, &pose.matrix());
            if !tree.inside {
                continue;
            }
            // The trunk stops tanks and rounds; the crown is decoration.
            if let Some(near) = solids.iter().find(|(_, center, size, yaw)| {
                beyond_rect(tree.at, *center, *size, *yaw, 0.0) < 0.6
            }) {
                panic!("{name} at {:?} stands against {}", tree.at, near.0);
            }
            let (center, size) = super::blocking_shells(vec3(0.0, 0.6, 0.0), vec3(0.5, 1.2, 0.5));
            self.collider(root, &format!("{name} collision"), center, size);
        }
    }

    /// The windmill, `Arena Cover Windmill`, in its footprint, with one
    /// solid round its tower and plinth.
    fn windmill(&mut self) {
        let (_, x0, z0, x1, z1) = layout::FOOTPRINTS
            .iter()
            .copied()
            .find(|footprint| footprint.0 == "Windmill")
            .unwrap();
        assert!(
            (x0..x1).contains(&WINDMILL.x) && (z0..z1).contains(&WINDMILL.y),
            "the windmill stands in its footprint"
        );
        let root = self.kit(
            "windmill",
            "Arena Cover Windmill",
            vec3(WINDMILL.x, 0.0, WINDMILL.y),
            WINDMILL_YAW,
        );
        let (center, size) = super::blocking_shells(
            vec3(0.0, 1.0, 0.0),
            vec3(WINDMILL_SOLID, 2.0, WINDMILL_SOLID),
        );
        self.collider(root, "Arena Cover Windmill collision 0", center, size);
    }

    /// The ground footprints of the arena's solids so far, `(name, centre,
    /// size, yaw)`: every box collider but the drive surfaces and the brush.
    fn solids(&self) -> Vec<(String, Vec2, Vec2, f32)> {
        let scene = &self.scene;
        // Solids' poses in the Arena's frame.
        let arena = scene.world_transform(self.arena).position;
        scene
            .descendants(self.arena)
            .filter(|&object| {
                scene
                    .read_component::<ComponentDriveSurface, _, _>(object, |_| ())
                    .is_none()
                    && scene
                        .ancestor_with::<sandbox::brush::ComponentBrush>(object)
                        .is_none()
            })
            .filter_map(|object| {
                let ColliderShape::Cuboid { half_extents } = scene
                    .read_component::<ComponentCollider, _, _>(object, |collider| collider.shape)?
                else {
                    return None;
                };
                let pose = scene.world_transform(object);
                let x = pose.rotation * Vec3::x();
                let yaw = (-x.z).atan2(x.x).to_degrees();
                Some((
                    scene.name(object).to_string(),
                    vec2(pose.position.x - arena.x, pose.position.z - arena.z),
                    vec2(half_extents.x * 2.0, half_extents.z * 2.0),
                    yaw,
                ))
            })
            .collect()
    }
}
