//! The arena's asymmetric layout: a creek forks in the middle of the pen
//! (see `farmland.rs`) and splits it into three, the south team's bank in
//! the southwest, the north team's in the northwest and the contested east
//! between the branches, crossed by three bridges at uneven positions. Each
//! corner has its own character:
//!
//! - **Southwest, the farm**: the ruined farmhouse, the water tower, a field
//!   and hedgerows, beside the creek's deep bend into the south half.
//! - **Southeast, the supply depot**: a walled yard of stacked crates in the
//!   corner, past the south branch, dressed later by #581.
//! - **Northwest, the orchard**: rows of trees broken by hedgerows and a
//!   wall, with a windmill's footprint in the corner, dressed later by #580.
//! - **Northeast, the barn yard**: the raised platform, the barn against
//!   its south ramp, the silo in the east cove, an open feed yard at the
//!   ramp's foot, and a stable and a paddock in the wide lobe north of the
//!   platform, past the north branch (`barn_yard.rs`).
//!
//! Interior walls, hedgerows and lines of cover run at 35 to 45 degrees to
//! the arena's axes, each at its own angle: nothing mirrors between the
//! sections. Positions are in the Arena's frame, where the south spawn is
//! at (0, -22) and the north spawn at [`NORTH_SPAWN`].

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ColliderShape, ComponentCollider, ComponentFoliage, ComponentGrassField, Orientation,
};
use calyx_engine::math::Transform;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};

use super::{asset_id, Author};

/// The north team's spawn, facing south across the creek: on its own bank,
/// as far from the map's centre and its nearest bridge as the south spawn
/// at (0, -22) is from them (see the fairness test in
/// `tests/feature_regressions.rs`), and not its mirror.
pub const NORTH_SPAWN: Vec2 = Vec2::new(-10.0, 18.5);

/// Ground the corner ETs dress, `(name, x0, z0, x1, z1)`: kept clear of
/// cover here except for the yard cover that already marks each corner.
/// The barn and silo (#579), the windmill among the orchard rows (#580) and
/// the supply depot's sheds (#581) go inside them.
pub const FOOTPRINTS: [(&str, f32, f32, f32, f32); 5] = [
    // The barn's feed yard, in front of its door, and the silo's pad (see
    // `barn_yard.rs`).
    ("Barn", 12.5, 6.0, 21.5, 12.5),
    ("Silo", 28.5, 12.0, 33.5, 18.0),
    ("Windmill", -27.0, 24.5, -22.0, 29.5),
    ("Orchard", -27.0, 12.0, -13.5, 29.0),
    ("Depot", 18.5, -29.0, 28.0, -17.0),
];

/// The sand lanes, as points along their middles, each smoothed into a
/// curve: from the south spawn over the west bridge and on to the north
/// spawn, from the south spawn over the south bridge and on through the
/// east to the platform's south ramp, and from the north spawn over the
/// north bridge to the west ramp.
pub const LANES: [&[(f32, f32)]; 3] = [
    &[
        (0.0, -20.0),
        (-2.5, -14.0),
        (-5.5, -9.0),
        (-9.0, -4.2),
        (-8.0, 2.0),
        (-7.0, 9.0),
        (-8.5, 13.0),
        (-10.0, 17.5),
    ],
    &[
        (3.0, -21.0),
        (7.0, -18.0),
        (11.0, -15.5),
        (14.5, -12.5),
        (19.0, -5.0),
        (20.0, 2.5),
        (20.0, 10.5),
    ],
    &[
        (-8.5, 19.5),
        (-4.5, 22.0),
        (-0.8, 23.5),
        (3.5, 23.5),
        (6.5, 24.0),
    ],
];
/// Half a lane's width.
pub const LANE_HALF_WIDTH: f32 = 1.7;

/// The straight interior walls, `(name, x, z, yaw, long modules)`: an end
/// piece rooted at `x`, `z`, then the long modules and an end piece along
/// the wall's local X turned `yaw` degrees. A 7.2 m wall has one long
/// module. Each keeps a gap wider than a tank, at least 2.8 m, to other
/// obstacles and 4.8 m to the perimeter.
const WALLS: [(&str, f32, f32, f32, usize); 5] = [
    // Breaks the long sightline across the east field, between the south
    // bridge and the barn yard.
    ("East Field", 23.0, -3.0, 40.0, 1),
    // Closes the depot's yard from the north-west.
    ("Depot", 21.0, -14.5, 38.0, 1),
    // Shields the farm's field and water tower from the spawn's lane.
    ("Farm", -21.0, -16.5, 47.0, 1),
    // The orchard's edge towards the creek.
    ("Orchard", -17.0, 9.5, 135.0, 1),
    // Screens the barn yard from the fork.
    ("Barn Yard", 13.5, 3.0, -43.0, 1),
];

/// Hedgerows, `(name, x, z, yaw)`.
const HEDGEROWS: [(&str, f32, f32, f32); 4] = [
    ("Farm Lane", -6.5, -23.5, -44.0),
    ("Creek Field", -23.5, -12.0, 37.0),
    ("Depot", 21.5, -9.5, 44.0),
    ("Orchard", -15.5, 17.5, 42.0),
];

/// Cover, `(model, name, x, z, yaw)`: lines of barriers, sandbags and crates
/// at the walls' angles, and rocks.
const COVER: [(&str, &str, f32, f32, f32); 15] = [
    ("barrier_hazard", "Spawn Barrier", 2.5, -10.5, 40.0),
    ("barrier_plain", "Creek Barrier", 15.0, -1.5, -35.0),
    ("crate_framed", "Creek Supply", 16.0, -6.0, 45.0),
    ("crate_large", "Creek Crate", 17.0, -7.0, 45.0),
    ("crate_large", "Depot Stack 0", 22.6, -25.1, 37.0),
    ("crate_large", "Depot Stack 1", 20.9, -23.8, 37.0),
    ("crate_framed", "Depot Stack 2", 27.5, -21.0, 52.0),
    ("barrier_plain", "Orchard Barrier", -12.5, 4.5, -38.0),
    ("sandbag_stack", "Orchard Sandbags", -13.0, 6.5, -38.0),
    ("crate_large", "Barn Crate", 22.5, 30.0, 38.0),
    ("barrier_hazard", "Barn Barrier", 31.5, 23.5, 40.0),
    ("barrier_plain", "East Barrier", 25.0, 8.0, -40.0),
    ("rock_large", "Spawn Boulder", -3.5, -16.5, 20.0),
    ("rock_large", "Orchard Boulder", -22.5, 6.5, 60.0),
    ("rock_small", "Farm Stone", -17.0, -11.5, 0.0),
];

/// The orchard's rows: trees `ORCHARD_SPACING` apart along rows turned
/// `ORCHARD_YAW` degrees, the rows `ORCHARD_ROW_GAP` apart, round
/// `ORCHARD_CENTER`, inside the orchard's footprint and clear of its
/// hedgerow, its brush and the windmill's footprint: dense rows that break
/// sight into short lanes, each gap wider than a tank.
const ORCHARD_CENTER: Vec2 = Vec2::new(-19.25, 15.5);
const ORCHARD_YAW: f32 = 37.0;
const ORCHARD_SPACING: f32 = 3.7;
const ORCHARD_ROW_GAP: f32 = 3.9;

/// One piece of a brush patch's outline, in the patch's frame: a box
/// `size` across its local X and Z turned `yaw` degrees, or a circle.
#[derive(Clone, Copy)]
pub enum BrushPart {
    Box {
        center: (f32, f32),
        size: (f32, f32),
        yaw: f32,
    },
    Circle {
        center: (f32, f32),
        radius: f32,
    },
}

impl BrushPart {
    /// How far `p`, in the patch's frame, lies beyond this part: negative
    /// inside it.
    pub fn beyond(&self, p: Vec2) -> f32 {
        match *self {
            BrushPart::Box { center, size, yaw } => {
                beyond_rect(p, vec2(center.0, center.1), vec2(size.0, size.1), yaw, 0.0)
            }
            BrushPart::Circle { center, radius } => (p - vec2(center.0, center.1)).norm() - radius,
        }
    }
}

/// A patch of tall-grass brush (`sandbox::brush`): its name, its origin
/// and yaw in the Arena's frame, and its outline, the union of its parts.
/// Each part is one of the patch's volumes, so the grass and the ground a
/// tank hides on are the same shape.
pub struct BrushPatch {
    pub name: &'static str,
    pub origin: (f32, f32),
    pub yaw: f32,
    pub parts: &'static [BrushPart],
}

const fn circle(x: f32, z: f32, radius: f32) -> BrushPart {
    BrushPart::Circle {
        center: (x, z),
        radius,
    }
}

const fn strip(x: f32, z: f32, length: f32, width: f32, yaw: f32) -> BrushPart {
    BrushPart::Box {
        center: (x, z),
        size: (length, width),
        yaw,
    }
}

/// The patches, placed unevenly along the creek's banks, clear of the
/// water, and by the landmarks, each its own shape: a kidney and a blob of
/// circles by the creek's banks, a thin strip with a lobe in the east
/// field, an L wrapping the farmhouse's south-east corner, a teardrop in
/// the orchard, a peanut by the barn yard and a crescent round the depot
/// wall's end. Each is clear of cover.
pub const BRUSH_PATCHES: [BrushPatch; 7] = [
    BrushPatch {
        name: "Creek West",
        origin: (-20.5, 1.8),
        yaw: 20.0,
        parts: &[
            circle(-2.2, 0.5, 1.5),
            circle(-0.2, -0.2, 1.8),
            circle(1.9, 0.4, 1.4),
            circle(3.1, 1.6, 1.0),
        ],
    },
    BrushPatch {
        name: "Fork East",
        origin: (11.5, 7.0),
        yaw: 0.0,
        parts: &[
            circle(0.0, 0.0, 2.0),
            circle(1.7, 1.0, 1.3),
            circle(-1.5, 0.9, 1.2),
            circle(0.5, -1.5, 1.1),
        ],
    },
    BrushPatch {
        name: "Yard East",
        origin: (14.2, -4.3),
        yaw: -6.0,
        parts: &[strip(0.0, 0.0, 5.6, 1.4, 0.0), circle(1.6, -0.6, 1.0)],
    },
    BrushPatch {
        name: "Farmyard",
        origin: (-13.5, -22.0),
        yaw: 0.0,
        parts: &[
            strip(0.0, 0.4, 5.0, 1.6, 0.0),
            strip(1.7, -1.4, 1.6, 3.6, 0.0),
        ],
    },
    BrushPatch {
        name: "Orchard",
        origin: (-15.5, 13.0),
        yaw: 40.0,
        parts: &[
            circle(-1.0, 0.0, 1.7),
            circle(0.9, 0.2, 1.2),
            circle(2.2, 0.3, 0.8),
        ],
    },
    BrushPatch {
        name: "Barn",
        origin: (11.5, 20.5),
        yaw: -35.0,
        parts: &[
            circle(-1.6, 0.0, 1.3),
            strip(0.0, 0.0, 3.0, 1.6, 0.0),
            circle(1.7, 0.2, 1.1),
        ],
    },
    BrushPatch {
        name: "Depot",
        origin: (25.0, -16.0),
        yaw: 30.0,
        parts: &[
            circle(-1.6, 1.2, 1.0),
            circle(-0.6, -0.2, 1.1),
            circle(0.8, -0.8, 1.1),
            circle(2.0, -0.2, 0.9),
        ],
    },
];

impl BrushPatch {
    /// The Arena-frame point `p` in the patch's frame.
    pub fn local(&self, p: Vec2) -> Vec2 {
        let (sin, cos) = self.yaw.to_radians().sin_cos();
        let d = p - vec2(self.origin.0, self.origin.1);
        vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    }

    /// How far the patch-frame point `p` lies beyond the outline: negative
    /// inside it.
    pub fn beyond_local(&self, p: Vec2) -> f32 {
        self.parts
            .iter()
            .map(|part| part.beyond(p))
            .fold(f32::MAX, f32::min)
    }

    /// How far the Arena-frame point `p` lies beyond the outline.
    pub fn beyond(&self, p: Vec2) -> f32 {
        self.beyond_local(self.local(p))
    }

    /// The patch-frame rectangle round the outline: its least and greatest
    /// corners.
    fn bounds(&self) -> (Vec2, Vec2) {
        let mut low = vec2(f32::MAX, f32::MAX);
        let mut high = vec2(f32::MIN, f32::MIN);
        for part in self.parts {
            let (center, reach) = match *part {
                BrushPart::Box { center, size, .. } => (center, size.0.hypot(size.1) * 0.5),
                BrushPart::Circle { center, radius } => (center, radius),
            };
            low = low.inf(&vec2(center.0 - reach, center.1 - reach));
            high = high.sup(&vec2(center.0 + reach, center.1 + reach));
        }
        (low, high)
    }

    /// Where the patch's clumps stand, in its frame: one to each cell of an
    /// even grid whose clump area, `(center, side, edge)`, lies wholly
    /// inside the outline. A cell whose full area leaves the outline gets
    /// a smaller area if that fits, and is `edge`; a third of the edge
    /// cells are left out, by hash, so the outline reads ragged rather than
    /// cut.
    pub fn cells(&self) -> Vec<(Vec2, f32, bool)> {
        /// The side of the grid cells, in metres.
        const CELL: f32 = 0.9;
        let (low, high) = self.bounds();
        let fits = |center: Vec2, side: f32| {
            let h = side * 0.5;
            [(-h, -h), (h, -h), (h, h), (-h, h)]
                .iter()
                .all(|&(x, z)| self.beyond_local(center + vec2(x, z)) < 0.0)
        };
        let mut cells = Vec::new();
        let columns = ((high.x - low.x) / CELL).ceil() as i32;
        let rows = ((high.y - low.y) / CELL).ceil() as i32;
        for column in 0..columns {
            for row in 0..rows {
                let center = low + vec2((column as f32 + 0.5) * CELL, (row as f32 + 0.5) * CELL);
                if fits(center, CELL * 0.8) {
                    cells.push((center, CELL * 0.8, false));
                } else if fits(center, CELL * 0.4) {
                    let hash = (column * 73_856_093) ^ (row * 19_349_663) ^ self.name.len() as i32;
                    if hash.rem_euclid(3) != 0 {
                        cells.push((center, CELL * 0.4, true));
                    }
                }
            }
        }
        cells
    }
}

/// How far `p` lies beyond a rectangle grown by `margin`, centred on
/// `center`, `size` across its local X and Z, turned `yaw` degrees:
/// negative inside it.
pub fn beyond_rect(p: Vec2, center: Vec2, size: Vec2, yaw: f32, margin: f32) -> f32 {
    let (sin, cos) = yaw.to_radians().sin_cos();
    let d = p - center;
    // Local X is (cos, -sin) and local Z (sin, cos) on the ground.
    let local = vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
    let edge = vec2(local.x.abs() - size.x * 0.5, local.y.abs() - size.y * 0.5);
    edge.x.max(edge.y) - margin
}

/// The orchard's tree positions.
pub fn orchard_trees() -> Vec<Vec2> {
    let (sin, cos) = ORCHARD_YAW.to_radians().sin_cos();
    let (along, across) = (vec2(cos, -sin), vec2(sin, cos));
    let mut trees = Vec::new();
    for row in -5..=5 {
        for column in -5..=5 {
            let p = ORCHARD_CENTER
                + along * (column as f32 * ORCHARD_SPACING)
                + across * (row as f32 * ORCHARD_ROW_GAP);
            let inside = p.x > -25.2 && p.x < -14.5 && p.y > 11.5 && p.y < 27.2;
            let (_, x0, z0, x1, z1) = FOOTPRINTS[2];
            let windmill = p.x > x0 - 1.0 && p.x < x1 + 1.0 && p.y > z0 - 1.0 && p.y < z1 + 1.0;
            let clear = HEDGEROWS
                .iter()
                .all(|&(_, x, z, yaw)| beyond_rect(p, vec2(x, z), vec2(4.4, 1.3), yaw, 3.0) > 0.0)
                && BRUSH_PATCHES.iter().all(|patch| patch.beyond(p) > 1.8);
            if inside && clear && !windmill {
                trees.push(p);
            }
        }
    }
    trees
}

/// The point on a lane's smoothed curve: Catmull-Rom through its points,
/// the ends extended by reflecting their neighbours.
pub fn lane_curve(points: &[(f32, f32)]) -> Vec<Vec2> {
    let at = |i: isize| -> Vec2 {
        let n = points.len() as isize;
        let p = |i: isize| vec2(points[i as usize].0, points[i as usize].1);
        if i < 0 {
            p(0) * 2.0 - p(1)
        } else if i >= n {
            p(n - 1) * 2.0 - p(n - 2)
        } else {
            p(i)
        }
    };
    let mut out = Vec::new();
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for step in 0..16 {
            let u = step as f32 / 16.0;
            let (u2, u3) = (u * u, u * u * u);
            out.push(
                (p1 * 2.0
                    + (p2 - p0) * u
                    + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2
                    + (p3 - p0 + (p1 - p2) * 3.0) * u3)
                    * 0.5,
            );
        }
    }
    out.push(at(points.len() as isize - 1));
    out
}

/// The distance from `p` to the middle of the nearest lane.
pub fn lane_distance(curves: &[Vec<Vec2>], p: Vec2) -> f32 {
    let mut nearest = f32::MAX;
    for curve in curves {
        for pair in curve.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let ab = b - a;
            let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
            nearest = nearest.min((p - (a + ab * f)).norm());
        }
    }
    nearest
}

impl Author {
    /// The layout's walls, hedgerows, cover, orchard and brush.
    pub fn layout(&mut self) {
        for &(name, x, z, yaw, longs) in &WALLS {
            self.straight_wall(name, vec2(x, z), yaw, longs);
        }
        for &(name, x, z, yaw) in &HEDGEROWS {
            self.prop(
                "hedgerow",
                &format!("Arena Cover {name} Hedgerow"),
                x,
                z,
                yaw,
            );
        }
        for &(model, name, x, z, yaw) in &COVER {
            self.prop(model, &format!("Arena Cover {name}"), x, z, yaw);
        }
        for (i, tree) in orchard_trees().into_iter().enumerate() {
            let model = ["tree_small", "tree", "tree_small", "tree_large"][i % 4];
            self.prop(
                model,
                &format!("Arena Prop Orchard Tree {i}"),
                tree.x,
                tree.y,
                (i * 41) as f32,
            );
        }
        self.brush();
    }

    /// A straight interior wall, `Arena Wall {name}`: an end piece rooted at
    /// `start`, `longs` long modules and an end piece along its local X,
    /// turned `yaw` degrees, with one solid along its whole run.
    fn straight_wall(&mut self, name: &str, start: Vec2, yaw: f32, longs: usize) {
        let mut pieces = vec![("wall_end", 0.0, 0.0, 0.0)];
        for i in 0..longs {
            pieces.push(("wall_long", 3.0 + 4.0 * i as f32, 0.0, 0.0));
        }
        let last = 4.0 * longs as f32 + 2.0;
        pieces.push(("wall_end", last, 0.0, 180.0));
        let run = (vec2(-0.6, 0.0), vec2(last + 0.6, 0.0));
        self.interior_wall_at(name, start, yaw, &pieces, &[run]);
    }

    /// The patches of tall-grass brush (`sandbox::brush`), see
    /// [`BRUSH_PATCHES`]: each a Brush with a box volume that stops nothing,
    /// 2 m high on the ground, and its grass a Foliage of the kit's
    /// `tall_grass` clumps packed over it. The Arena's Grass Field sways all
    /// of them and bends them around tanks.
    fn brush(&mut self) {
        // The kit's tall blades arc over by drooping in the grass shader.
        let grass_field = || ComponentGrassField {
            droop: 0.32,
            ..Default::default()
        };
        let prefab = asset_id(&self.assets.join(format!(
            "generated/prefabs/{}/tall_grass.cxprefab",
            self.palette.folder()
        )));
        for (i, patch) in BRUSH_PATCHES.iter().enumerate() {
            let name = format!("Arena Brush {}", patch.name);
            let origin = vec3(patch.origin.0, 0.0, patch.origin.1);
            let root = self.object(&name, self.arena, origin);
            self.scene.set_transform(
                root,
                &Transform::from_components(
                    origin,
                    UnitQuaternion::from_euler_angles(0.0, patch.yaw.to_radians(), 0.0),
                    vec3(1.0, 1.0, 1.0),
                )
                .matrix(),
            );
            self.scene
                .add_component(root, sandbox::brush::ComponentBrush::default());
            // One clump to each cell inside the outline, placed at random in
            // its area and turned at random, so the blades fill the patch
            // evenly without showing the grid; the smaller edge cells carry
            // shorter clumps, so the patch thins out to its edge.
            for (j, (center, side, edge)) in patch.cells().into_iter().enumerate() {
                let cell = self.object(
                    &format!("{name} Grass {j}"),
                    root,
                    vec3(center.x, 0.0, center.y),
                );
                let area = vec2(side, side);
                self.scene.add_component(
                    cell,
                    ComponentFoliage {
                        prefabs: vec![AssetRef::from_id(prefab)],
                        area,
                        // Exactly one clump in the cell.
                        density: 1.05 / (area.x * area.y),
                        seed: 575 + (i as u32) * 97 + j as u32,
                        scale_range: if edge { vec2(0.6, 0.8) } else { vec2(0.9, 1.1) },
                        rotation_range: vec2(0.0, 360.0),
                        // Shadow maps would draw every blade again in
                        // each cascade, for shadows the grass mostly
                        // falls on itself; SSAO still grounds it.
                        cast_shadows: false,
                    },
                );
            }
            // The volumes: each part of the outline, 2 m high on the ground,
            // a box or an upright capsule, so the ground a tank hides on is
            // the outline the grass fills.
            for (j, part) in patch.parts.iter().enumerate() {
                let volume = match *part {
                    BrushPart::Box { center, size, yaw } => self.tilted_collider(
                        root,
                        &format!("{name} Volume {j}"),
                        vec3(center.0, 1.0, center.1),
                        vec3(size.0, 2.0, size.1),
                        UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                    ),
                    BrushPart::Circle { center, radius } => {
                        let volume = self.collider(
                            root,
                            &format!("{name} Volume {j}"),
                            vec3(center.0, 1.0, center.1),
                            Vec3::repeat(1.0),
                        );
                        self.scene
                            .write_component::<ComponentCollider, _>(volume, |collider| {
                                collider.shape = ColliderShape::Capsule {
                                    orientation: Orientation::Y,
                                    height: 1.0,
                                    radius,
                                };
                            });
                        volume
                    }
                };
                self.scene
                    .write_component::<ComponentCollider, _>(volume, |collider| {
                        collider.groups = sandbox::collision::BRUSH;
                    });
            }
        }
        if self
            .scene
            .write_component::<ComponentGrassField, _>(self.arena, |field| {
                *field = grass_field();
            })
            .is_none()
        {
            self.scene.add_component(self.arena, grass_field());
        }
    }
}
