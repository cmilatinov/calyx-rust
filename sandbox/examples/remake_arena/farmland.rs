//! The southwest quadrant as a farmland front: a sunken creek that comes in
//! under the west wall, bends past the farm and leaves under the south
//! wall, crossed by a bridge on the west sand lane; a ruined farmhouse, a
//! water tower, hedgerows and fields of crop rows; and fields, fences and
//! trees outside the walls.
//!
//! Every position is in the Arena's frame, where the pen spans x -30..30
//! and z -32..32 and the spawn is at (0, -22).

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ComponentCollider, ComponentMesh, ComponentWater, WaterObstacle, WaterPoint, WaterShading,
};
use calyx_engine::math::{Transform, WaterSection};
use calyx_engine::scene::GameObject;
use calyx_engine::utils::TypeUuid;
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::collision::TANK_BLOCKER;

use calyx_model_gen::palette::Map;

use super::{asset_id, Author};

/// The water's rest surface, below the ground: the creek is sunken.
pub const WATER_Y: f32 = -0.6;
/// The banks' lip, just above the ground tiles' tops at y = 0 so the two
/// never fight.
const BANK_TOP: f32 = 0.02;
const BANK_DEPTH: f32 = 0.6;
const BANK_RUN: f32 = 0.4;
const BANK_LIP: f32 = 0.5;
/// How far past the shore the trench reaches: the bank wall meets its lip
/// at `BANK_RUN * height / (depth + height)` past the shore, and ground
/// tiles stay this far clear so none pokes over the wall.
const TRENCH: f32 = 0.45;
/// Half a one-metre ground cell's diagonal.
const HALF_DIAGONAL: f32 = 0.71;
/// Heights of the ground overlays above the tiles' tops: the verge along
/// the creek and, over it, the crop rows.
const VERGE_TOP: f32 = 0.006;
const CROP_TOP: f32 = 0.012;
/// How far past the shore a creek blocker reaches: tanks stop with their
/// hull over the bank's lip, short of the water.
const BLOCKER_MARGIN: f32 = 0.5;
/// The creek blockers rise from the water to above the hull.
const BLOCKER_BOTTOM: f32 = WATER_Y;
const BLOCKER_TOP: f32 = 1.4;

/// The creek's centreline, from upstream (west, outside the pen) to
/// downstream (south, outside the pen), and its width at each point: 6 to
/// 8 m, with bends about 15 to 20 m apart.
const CREEK: [(f32, f32, f32); 12] = [
    (-60.0, -26.0, 7.0),
    (-46.0, -16.0, 8.0),
    (-34.0, -17.0, 7.5),
    (-26.0, -20.5, 6.5),
    (-20.0, -21.5, 5.5),
    (-14.0, -23.0, 6.5),
    (-10.5, -26.0, 6.5),
    (-11.0, -29.5, 6.5),
    (-13.5, -33.0, 6.5),
    (-10.0, -42.0, 8.0),
    (-16.0, -54.0, 7.5),
    (-12.0, -66.0, 7.0),
];

/// The bridge carries the west sand lane, centred on this x, over the creek.
pub const BRIDGE_X: f32 = -20.0;
/// Half the clear width between the bridge's rails, and their thickness.
const BRIDGE_HALF_WIDTH: f32 = 1.9;
const RAIL_THICKNESS: f32 = 0.3;
const BRIDGE_LENGTH: f32 = 9.0;

/// A rectangle of ground on the one-metre grid: x from `x0` to `x1` and z
/// from `z0` to `z1`, in whole metres.
#[derive(Clone, Copy)]
pub struct Rect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
}

impl Rect {
    const fn new(x0: i32, z0: i32, x1: i32, z1: i32) -> Self {
        Self { x0, z0, x1, z1 }
    }

    fn contains(&self, x: i32, z: i32) -> bool {
        (self.x0..self.x1).contains(&x) && (self.z0..self.z1).contains(&z)
    }
}

/// A tilled field: its rectangle, and whether its crop rows run along X
/// (else along Z).
struct Field {
    rect: Rect,
    rows_along_x: bool,
}

const FIELDS: [Field; 5] = [
    // Beside the farmhouse, north of the spawn.
    Field {
        rect: Rect::new(-6, -17, -2, -7),
        rows_along_x: false,
    },
    // West of the lane, north of the creek.
    Field {
        rect: Rect::new(-28, -14, -23, -7),
        rows_along_x: true,
    },
    // Outside, beyond the west wall and the south wall.
    Field {
        rect: Rect::new(-46, -9, -33, 6),
        rows_along_x: false,
    },
    Field {
        rect: Rect::new(-44, -46, -21, -35),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(-2, -47, 14, -35),
        rows_along_x: true,
    },
];

/// The water tower, in the pen's southwest corner.
const TOWER: Vec3 = Vec3::new(-26.0, 0.0, -28.0);
/// The water tower's solid: its legs and footings, rising to the
/// shell-blocking top. The tower stands clear of the walls, its roof
/// included, leaving a gap too narrow for a tank.
pub const TOWER_SOLID: f32 = 3.0;

/// The creek's water and cross-sections, and the bridge over it.
pub struct Creek {
    pub water: ComponentWater,
    sections: Vec<WaterSection>,
    /// The section under the middle of the bridge.
    bridge: WaterSection,
    /// The z where the bridge's deck is centred.
    bridge_z: f32,
}

impl Creek {
    /// The creek in `palette`'s water colours, as the direction lists them,
    /// with its banks in the palette's `creek_bank` material.
    pub fn new(assets: &std::path::Path, palette: Map) -> Self {
        let bank_material = AssetRef::from_id(asset_id(&assets.join(format!(
            "generated/materials/{}/creek_bank.cxmat",
            palette.folder()
        ))));
        let (deep_color, shallow_color, foam_color, glint_strength) = match palette {
            Map::SunnyBattlefield => (
                Color32::from_rgb(0x2F, 0x7C, 0xA0),
                Color32::from_rgb(0x5D, 0xB4, 0xBF),
                Color32::from_rgb(0xE6, 0xF4, 0xEE),
                1.5,
            ),
            Map::Sunbaked => (
                Color32::from_rgb(0x2E, 0x74, 0x7A),
                Color32::from_rgb(0x6A, 0xA8, 0x9C),
                Color32::from_rgb(0xEC, 0xE6, 0xD6),
                1.5,
            ),
            Map::Moonlit => (
                Color32::from_rgb(0x1B, 0x35, 0x4E),
                Color32::from_rgb(0x2C, 0x5A, 0x73),
                Color32::from_rgb(0x9C, 0xBD, 0xCC),
                5.0,
            ),
        };
        let points = CREEK
            .iter()
            .map(|&(x, z, width)| WaterPoint {
                position: vec2(x, z),
                width,
            })
            .collect();
        let mut water = ComponentWater {
            points,
            cell_size: 0.8,
            shading: WaterShading::Smooth,
            deep_color,
            shallow_color,
            foam_color,
            glint_strength,
            bank_material,
            bank_height: BANK_TOP - WATER_Y,
            bank_depth: BANK_DEPTH,
            bank_run: BANK_RUN,
            bank_lip: BANK_LIP,
            ..Default::default()
        };
        let sections = water.path().sections().expect("the creek meshes");
        let bridge = *sections
            .iter()
            .min_by(|a, b| {
                (a.center.x - BRIDGE_X)
                    .abs()
                    .total_cmp(&(b.center.x - BRIDGE_X).abs())
            })
            .unwrap();
        let bridge_z = bridge.center.y;
        // Foam rings the stones standing in the stream.
        water.obstacles = STREAM_ROCKS
            .iter()
            .map(|&(s, across, scale)| {
                let (position, _) = along(&sections, s, across);
                WaterObstacle {
                    position,
                    radius: 0.55 * scale,
                }
            })
            .collect();
        Self {
            water,
            sections,
            bridge,
            bridge_z,
        }
    }

    /// How far `p` lies beyond the creek's nearest shore: negative over the
    /// water.
    pub fn beyond(&self, p: Vec2) -> f32 {
        let mut beyond = f32::MAX;
        for pair in self.sections.windows(2) {
            let (a, b) = (pair[0].center, pair[1].center);
            let ab = b - a;
            let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
            let half = pair[0].half_width + (pair[1].half_width - pair[0].half_width) * f;
            beyond = beyond.min((p - (a + ab * f)).norm() - half);
        }
        beyond
    }

    /// Whether the one-metre ground cell at `x`, `z` reaches into the
    /// trench, so it is left out of the ground.
    pub fn carves(&self, x: i32, z: i32) -> bool {
        self.beyond(vec2(x as f32 + 0.5, z as f32 + 0.5)) < TRENCH + HALF_DIAGONAL
    }
}

/// Stones standing in the stream: distance along the creek, share of its
/// half width across it (left positive) and scale: two west of the bridge,
/// one on the bend past the farm and one outside the south wall.
const STREAM_ROCKS: [(f32, f32, f32); 4] = [
    (24.0, 0.45, 1.0),
    (31.0, -0.5, 1.3),
    (51.0, 0.55, 1.1),
    (76.0, 0.4, 1.2),
];

/// The point `s` along the creek and `across` its half width to the left,
/// and the flow's yaw there in degrees.
fn along(sections: &[WaterSection], s: f32, across: f32) -> (Vec2, f32) {
    let section = sections
        .iter()
        .min_by(|a, b| (a.s - s).abs().total_cmp(&(b.s - s).abs()))
        .unwrap();
    (
        section.center + section.normal * section.half_width * across,
        section.tangent.x.atan2(section.tangent.y).to_degrees(),
    )
}

/// The ground kind of the one-metre cell at `x`, `z` in the farmland, or
/// `None` where the farmland leaves the cell as it was.
pub fn farm_ground(x: i32, z: i32) -> Option<&'static str> {
    let outside = !(-30..30).contains(&x) || !(-32..32).contains(&z);
    if FIELDS.iter().any(|field| field.rect.contains(x, z)) {
        return Some("field");
    }
    if outside {
        return None;
    }
    // The farmland is the southwest quadrant, and its sand lanes stay sand.
    if x >= 0 || z >= 0 {
        return None;
    }
    let lane = (-22..-18).contains(&x) && z >= -27 || (-5..-2).contains(&z) && x >= -22;
    if lane || ((-3..3).contains(&x) && (-26..-20).contains(&z)) {
        return None;
    }
    Some("grass")
}

impl Author {
    /// The farmland's pieces above the ground tiles: the creek and its
    /// blockers and bridge, the farmhouse and water tower, hedgerows, crop
    /// rows and the outside's fences and trees.
    pub fn farmland(&mut self, creek: &Creek) {
        self.creek(creek);
        self.bridge(creek);
        self.farmhouse();
        self.prop_at("water_tower", "Arena Prop Farm Water Tower", TOWER, 0.0);
        for (name, x, z, yaw) in [
            ("Field Hedgerow", -5.0, -18.5, 0.0),
            ("West Field Hedgerow", -23.0, -12.0, 90.0),
        ] {
            self.prop("hedgerow", &format!("Arena Cover {name}"), x, z, yaw);
        }
        // Round bales against the farmhouse's east wall.
        for (i, z) in [-12.0, -10.76].into_iter().enumerate() {
            self.prop(
                "hay_bale",
                &format!("Arena Cover Farm Hay Bale {i}"),
                -6.8,
                z,
                0.0,
            );
        }
        self.crop_rows(creek);
        self.outside(creek);
    }

    /// The water, the stones standing in it and the reeds on its banks, and
    /// the blockers that keep tanks out of it: boxes about 1.6 m long along
    /// the centreline, each as wide as the water and a little more,
    /// overlapping the next so no tank slips between them, and short enough
    /// that on a bend their corners stay close to the banks. Rounds and
    /// sight cross them.
    fn creek(&mut self, creek: &Creek) {
        let water = self.object("Arena Creek", self.arena, vec3(0.0, WATER_Y, 0.0));
        self.scene.add_component(water, creek.water.clone());
        for (i, &(s, across, scale)) in STREAM_ROCKS.iter().enumerate() {
            let (at, yaw) = along(&creek.sections, s, across);
            let rock = self.kit(
                "rock_medium",
                &format!("Arena Creek Stone {i}"),
                vec3(at.x, WATER_Y - 0.55 * scale, at.y),
                yaw + 25.0 * i as f32,
            );
            self.scale(rock, scale);
        }
        let (bridge_lo, bridge_hi) = (
            BRIDGE_X - BRIDGE_HALF_WIDTH - RAIL_THICKNESS,
            BRIDGE_X + BRIDGE_HALF_WIDTH + RAIL_THICKNESS,
        );
        let in_pen = |p: Vec2| p.x.abs() < 29.5 && p.y.abs() < 31.5;
        // A section's blocker reaches this far either side of its centre.
        let reach = |section: &WaterSection| section.half_width + BLOCKER_MARGIN;
        let on_bridge = |section: &WaterSection| {
            [-1.0, 1.0].iter().any(|side| {
                let edge = section.center + section.normal * reach(section) * *side;
                let lo = section.center.x.min(edge.x) - 0.5;
                let hi = section.center.x.max(edge.x) + 0.5;
                hi > bridge_lo && lo < bridge_hi
            })
        };
        let mut chunks: Vec<Vec<WaterSection>> = vec![Vec::new()];
        for section in &creek.sections {
            if !in_pen(section.center) || on_bridge(section) {
                chunks.push(Vec::new());
                continue;
            }
            let chunk = chunks.last_mut().unwrap();
            chunk.push(*section);
            if (chunk[chunk.len() - 1].center - chunk[0].center).norm() >= 1.6 {
                chunks.push(vec![*section]);
            }
        }
        let block = |chunk: &[WaterSection]| {
            let (a, b) = (chunk[0], chunk[chunk.len() - 1]);
            let along = b.center - a.center;
            Block {
                center: (a.center + b.center) * 0.5,
                size: vec2(
                    chunk.iter().map(reach).fold(0.0, f32::max) * 2.0,
                    along.norm() + 0.5,
                ),
                yaw: along.x.atan2(along.y).to_degrees(),
            }
        };
        let blocks: Vec<Block> = chunks
            .iter()
            .filter(|chunk| chunk.len() >= 2)
            .map(|chunk| block(chunk))
            .collect();
        for (count, block) in blocks.iter().enumerate() {
            self.tank_blocker(
                &format!("Arena Creek Blocker {count}"),
                block.center,
                block.size,
                block.yaw,
            );
        }
        // Either side of the bridge, a block closes the creek from the last
        // blocker to the rail, whatever angle the water crosses the lane at.
        for (side, lo, hi) in [
            ("West", bridge_lo - 2.4, bridge_lo),
            ("East", bridge_hi, bridge_hi + 2.4),
        ] {
            let ends: Vec<f32> = creek
                .sections
                .iter()
                .filter(|section| {
                    (lo..=hi).contains(&section.center.x)
                        && (section.s - creek.bridge.s).abs() < 6.0
                })
                .flat_map(|section| {
                    [-1.0, 1.0].map(|s| (section.center + section.normal * reach(section) * s).y)
                })
                .collect();
            let z0 = ends.iter().copied().fold(f32::MAX, f32::min);
            let z1 = ends.iter().copied().fold(f32::MIN, f32::max);
            let (x0, x1) = if side == "West" {
                (bridge_lo - 2.4, bridge_lo + RAIL_THICKNESS)
            } else {
                (bridge_hi - RAIL_THICKNESS, bridge_hi + 2.4)
            };
            self.tank_blocker(
                &format!("Arena Creek Blocker Bridge {side}"),
                vec2((x0 + x1) * 0.5, (z0 + z1) * 0.5),
                vec2(x1 - x0, z1 - z0),
                0.0,
            );
        }
        // Reeds along the banks, on the lip at the water's edge.
        for (i, &(s, side)) in [(27.5, -1.0), (46.5, -1.0), (60.0, -1.0), (70.0, 1.0)]
            .iter()
            .enumerate()
        {
            let section = creek
                .sections
                .iter()
                .min_by(|a, b| (a.s - s).abs().total_cmp(&(b.s - s).abs()))
                .unwrap();
            let at = section.center + section.normal * side * (section.half_width + 0.45);
            self.kit(
                "grass_tuft",
                &format!("Arena Grass Creek Reeds {i}"),
                vec3(at.x, BANK_TOP, at.y),
                (i * 53) as f32,
            );
        }
        // Grass along both banks, from under the lip out past the ground
        // cells the trench took, so no gap shows beside the lip.
        let mut verge = 0;
        for pair in creek.sections.chunks(4) {
            let (a, b) = (pair[0], pair[pair.len() - 1]);
            if (b.center - a.center).norm() < 0.1 {
                continue;
            }
            for side in [-1.0, 1.0] {
                let inner = 0.3;
                let outer = TRENCH + HALF_DIAGONAL * 2.0 + 0.2;
                let mid = |section: WaterSection| {
                    section.center
                        + section.normal * side * (section.half_width + (inner + outer) * 0.5)
                };
                let (pa, pb) = (mid(a), mid(b));
                let along = pb - pa;
                self.ground_strip(
                    "grass",
                    &format!("Arena Ground Creek Verge {verge}"),
                    (pa + pb) * 0.5,
                    vec2(outer - inner, along.norm() + 0.9),
                    along.x.atan2(along.y).to_degrees(),
                    VERGE_TOP,
                );
                verge += 1;
            }
        }
    }

    /// The bridge on the west sand lane, and its rails, which stop tanks
    /// but let rounds and sight across.
    fn bridge(&mut self, creek: &Creek) {
        let z = creek.bridge_z;
        self.kit(
            "bridge",
            "Arena Prop Farm Bridge",
            vec3(BRIDGE_X, 0.0, z),
            0.0,
        );
        for (i, side) in [-1.0, 1.0].into_iter().enumerate() {
            self.tank_blocker(
                &format!("Arena Bridge Rail {i}"),
                vec2(
                    BRIDGE_X + side * (BRIDGE_HALF_WIDTH + RAIL_THICKNESS * 0.5),
                    z,
                ),
                vec2(RAIL_THICKNESS, BRIDGE_LENGTH),
                0.0,
            );
        }
    }

    /// The ruined farmhouse: two corners of the kit's walls, its chimney
    /// still standing and its fallen roof, in a dirt yard by the creek.
    fn farmhouse(&mut self) {
        let run = |from: (f32, f32), to: (f32, f32)| (vec2(from.0, from.1), vec2(to.0, to.1));
        self.interior_wall(
            "Farmhouse",
            &[
                ("wall_corner", -14.0, -16.0, 0.0),
                ("wall_corner", -8.0, -10.0, 180.0),
            ],
            &[
                run((-14.6, -16.0), (-11.0, -16.0)),
                run((-14.0, -16.6), (-14.0, -13.0)),
                run((-11.0, -10.0), (-7.4, -10.0)),
                run((-8.0, -13.0), (-8.0, -9.4)),
            ],
        );
        let wall = self
            .scene
            .objects()
            .find(|o| self.scene.name(*o) == "Arena Wall Farmhouse")
            .unwrap();
        let chimney = vec3(-12.75, 0.0, -14.85);
        self.kit_under(
            wall,
            "chimney",
            "Arena Wall Farmhouse Chimney",
            chimney,
            0.0,
        );
        self.collider(
            wall,
            "Arena Wall Farmhouse Chimney collision",
            chimney + vec3(0.0, 1.9, 0.0),
            vec3(1.2, 3.8, 1.0),
        );
        self.kit(
            "roof_ruin",
            "Arena Prop Farmhouse Roof",
            vec3(-11.0, 0.0, -13.0),
            -12.0,
        );
    }

    /// Crop rows: stripes of green half a metre wide, a metre apart, along
    /// each field.
    fn crop_rows(&mut self, creek: &Creek) {
        let mut count = 0;
        for field in &FIELDS {
            let r = field.rect;
            let (across0, across1, along0, along1) = if field.rows_along_x {
                (r.z0, r.z1, r.x0 as f32, r.x1 as f32)
            } else {
                (r.x0, r.x1, r.z0 as f32, r.z1 as f32)
            };
            for row in across0..across1 {
                let across = row as f32 + 0.5;
                let (center, size, yaw) = if field.rows_along_x {
                    (
                        vec2((along0 + along1) * 0.5, across),
                        vec2(0.5, along1 - along0 - 0.6),
                        90.0,
                    )
                } else {
                    (
                        vec2(across, (along0 + along1) * 0.5),
                        vec2(0.5, along1 - along0 - 0.6),
                        0.0,
                    )
                };
                let half = vec2(0.0, size.y * 0.5);
                let half = if field.rows_along_x {
                    vec2(half.y, 0.0)
                } else {
                    half
                };
                assert!(
                    (0..=10).all(|i| {
                        creek.beyond(center - half + half * (i as f32 * 0.2)) > TRENCH + 1.0
                    }),
                    "crop row {count} runs into the creek"
                );
                self.ground_strip(
                    "crop",
                    &format!("Arena Ground Crop Row {count}"),
                    center,
                    size,
                    yaw,
                    CROP_TOP,
                );
                count += 1;
            }
        }
    }

    /// Outside the walls: fences along the fields, and trees in copses
    /// along the creek and at the fields' corners.
    fn outside(&mut self, creek: &Creek) {
        for (i, (x, z, yaw)) in [
            (-33.0, -7.0, 90.0),
            (-33.0, -3.0, 90.0),
            (-33.0, 1.0, 90.0),
            (-34.0, -34.5, 0.0),
            (-30.0, -34.5, 0.0),
            (-26.0, -34.5, 0.0),
            (-3.0, -34.5, 0.0),
            (1.0, -34.5, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            self.kit(
                "fence",
                &format!("Arena Prop Outside Fence {i}"),
                vec3(x, 0.0, z),
                yaw,
            );
        }
        let trees = [
            ("tree_large", -37.0, -23.5),
            ("tree_large", -33.5, -26.5),
            ("tree", -36.5, -29.0),
            ("tree", -40.5, -25.5),
            ("tree_large", -21.0, -37.5),
            ("tree", -19.0, -40.5),
            ("tree_small", -4.0, -37.5),
            ("tree_large", 17.0, -38.5),
        ];
        for (i, (model, x, z)) in trees.into_iter().enumerate() {
            assert!(
                creek.beyond(vec2(x, z)) > 2.0,
                "outside tree {i} stands in the creek"
            );
            self.kit(
                model,
                &format!("Arena Prop Outside Tree {i}"),
                vec3(x, 0.0, z),
                (i * 47) as f32,
            );
        }
    }

    /// A box collider turned `yaw` degrees, `size` across (x) and along
    /// (z), in the Arena's frame, that stops tanks only.
    fn tank_blocker(&mut self, name: &str, center: Vec2, size: Vec2, yaw: f32) {
        let object = self.tilted_collider(
            self.arena,
            name,
            vec3(center.x, (BLOCKER_BOTTOM + BLOCKER_TOP) * 0.5, center.y),
            vec3(size.x, BLOCKER_TOP - BLOCKER_BOTTOM, size.y),
            UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
        );
        self.scene
            .write_component::<ComponentCollider, _>(object, |collider| {
                collider.groups = TANK_BLOCKER;
            });
    }

    /// A ground tile of `kind`, `size` across (x) and along (z), turned
    /// `yaw` degrees, with its top `top` above the tiles'.
    fn ground_strip(
        &mut self,
        kind: &str,
        name: &str,
        center: Vec2,
        size: Vec2,
        yaw: f32,
        top: f32,
    ) {
        self.flat(
            &format!("ground_{kind}"),
            name,
            vec3(center.x, top - 0.1, center.y),
            yaw,
            vec3(size.x, 1.0, size.y),
        );
    }

    /// One plain object drawing the one-part `model`'s mesh in its
    /// material, at `position` turned `yaw` degrees and scaled by `scale`:
    /// the ground's overlays are many, so each is one object, not a prefab
    /// instance and its part.
    fn flat(&mut self, model: &str, name: &str, position: Vec3, yaw: f32, scale: Vec3) {
        let prefab = super::read(&self.assets.join(format!(
            "generated/prefabs/{}/{model}.cxprefab",
            self.palette.folder()
        )));
        let mesh_type = ComponentMesh::type_uuid().to_string();
        let mesh = prefab["data"]["components"]
            .as_object()
            .unwrap()
            .values()
            .find_map(|components| components.get(&mesh_type))
            .unwrap_or_else(|| panic!("{model} has no mesh"))
            .clone();
        let object = self.object(name, self.arena, position);
        self.scene.set_transform(
            object,
            &Transform::from_components(
                position,
                UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                scale,
            )
            .matrix(),
        );
        self.scene
            .add_component::<ComponentMesh>(object, serde_json::from_value(mesh).unwrap());
    }

    fn scale(&mut self, object: GameObject, scale: f32) {
        let mut pose = self.scene.transform(object);
        pose.scale = Vec3::repeat(scale);
        self.scene.set_transform(object, &pose.matrix());
    }
}

/// A box on the ground: its centre, its size across (its local x) and
/// along (its local z), and its yaw in degrees, which turns its local z
/// from +Z towards +X.
struct Block {
    center: Vec2,
    size: Vec2,
    yaw: f32,
}
