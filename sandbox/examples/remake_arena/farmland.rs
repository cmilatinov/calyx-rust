//! The farmland: a sunken creek that comes in under the west wall, bends
//! deep into the south half past the farm, then runs straighter through the
//! north half and leaves under the east wall, crossed by two bridges at
//! uneven positions; the ruined farmhouse, a water tower and fields of crop
//! rows in the southwest; and fields, fences and trees outside the walls.
//!
//! Every position is in the Arena's frame, where the pen spans x -30..30
//! and z -32..32 and the spawn is at (0, -22).

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ComponentCollider, ComponentWater, WaterObstacle, WaterPoint, WaterShading,
};
use calyx_engine::math::WaterSection;
use calyx_engine::scene::GameObject;
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::collision::TANK_BLOCKER;

use calyx_model_gen::palette::Map;

use super::{asset_id, Author};

/// The water's rest surface, below the ground: the creek is sunken.
pub const WATER_Y: f32 = -0.6;
/// The banks' lip, just above the ground at y = 0 so the two never fight.
const BANK_TOP: f32 = 0.02;
const BANK_DEPTH: f32 = 0.6;
const BANK_RUN: f32 = 0.4;
const BANK_LIP: f32 = 0.5;
/// How far past the shore the trench reaches: the bank wall meets its lip
/// at `BANK_RUN * height / (depth + height)` past the shore, and no
/// one-metre ground cell this close to the shore holds paving or road.
const TRENCH: f32 = 0.45;
/// Half a one-metre ground cell's diagonal.
const HALF_DIAGONAL: f32 = 0.71;
/// How far past the shore a creek blocker reaches: tanks stop with their
/// hull over the bank's lip, short of the water.
const BLOCKER_MARGIN: f32 = 0.5;
/// The creek blockers rise from the water to above the hull.
const BLOCKER_BOTTOM: f32 = WATER_Y;
const BLOCKER_TOP: f32 = 1.4;

/// The creek's centreline, from upstream (west, outside the pen) to
/// downstream (east, outside the pen), and its width at each point: 6 to
/// 8 m. Inside the pen it bends deep into the south half past the farm,
/// crosses the middle on a diagonal and runs straighter through the north
/// half to the east wall; outside, it keeps meandering to the apron's edge.
const CREEK: [(f32, f32, f32); 20] = [
    (-95.0, 13.0, 8.0),
    (-74.0, 8.0, 7.5),
    (-55.0, 11.0, 8.0),
    (-40.0, 6.5, 7.0),
    (-30.0, 1.5, 6.5),
    (-23.5, -4.5, 6.5),
    (-17.5, -8.0, 7.0),
    (-11.5, -6.0, 6.5),
    (-7.0, -2.8, 6.0),
    (-2.0, -0.4, 6.5),
    (4.0, 1.0, 7.0),
    (10.5, 1.5, 7.0),
    (16.0, 2.0, 6.5),
    (21.0, 2.5, 6.0),
    (26.0, 3.5, 6.5),
    (32.0, 6.5, 7.0),
    (42.0, 10.0, 7.5),
    (56.0, 7.0, 8.0),
    (73.0, 12.0, 7.5),
    (95.0, 9.0, 8.0),
];

/// The bridges, `(name, x)`: each stands on the creek where its centreline
/// crosses `x`, its deck square across the water. The west one crosses
/// the diagonal run near the centre in the open, the east one the straight
/// run far east, at the foot of the platform's south ramp.
pub const BRIDGES: [(&str, f32); 2] = [("West", -5.0), ("East", 20.0)];
/// Half the clear width between a bridge's rails, and their thickness.
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

const FIELDS: [Field; 6] = [
    // The farm's field, between the water tower and the creek.
    Field {
        rect: Rect::new(-27, -23, -23, -15),
        rows_along_x: false,
    },
    // Outside, beyond the west wall, the south wall, the north wall and
    // the east wall.
    Field {
        rect: Rect::new(-46, -17, -33, -3),
        rows_along_x: false,
    },
    Field {
        rect: Rect::new(-44, -48, -21, -38),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(-2, -47, 14, -35),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(-22, 35, -4, 45),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(34, -24, 46, -8),
        rows_along_x: false,
    },
];

/// The water tower, in the pen's southwest corner.
const TOWER: Vec3 = Vec3::new(-26.0, 0.0, -28.0);
/// The water tower's solid: its legs and footings, rising to the
/// shell-blocking top. The tower stands clear of the walls, its roof
/// included, leaving a gap too narrow for a tank.
pub const TOWER_SOLID: f32 = 3.0;

/// The creek's water and cross-sections, and the bridges over it.
pub struct Creek {
    pub water: ComponentWater,
    sections: Vec<WaterSection>,
    /// The section under the middle of each bridge, in [`BRIDGES`] order.
    pub bridges: Vec<WaterSection>,
}

/// A bridge's frame on the ground: centred on its deck, turned `yaw`
/// degrees so its local Z, the deck's length, runs along the water's
/// normal and its local X across the deck.
struct BridgeFrame {
    section: WaterSection,
    center: Vec2,
    yaw: f32,
    x_axis: Vec2,
    z_axis: Vec2,
}

impl BridgeFrame {
    fn new(section: &WaterSection) -> Self {
        let z_axis = section.normal;
        Self {
            section: *section,
            center: section.center,
            yaw: z_axis.x.atan2(z_axis.y).to_degrees(),
            x_axis: vec2(z_axis.y, -z_axis.x),
            z_axis,
        }
    }

    /// `p` in the bridge's frame: across its deck (x) and along it (y).
    fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        vec2(d.dot(&self.x_axis), d.dot(&self.z_axis))
    }

    /// The ground point at `local` in the bridge's frame.
    fn world(&self, local: Vec2) -> Vec2 {
        self.center + self.x_axis * local.x + self.z_axis * local.y
    }
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
        let bridges = BRIDGES.iter().map(|&(_, x)| at_x(&sections, x)).collect();
        // Foam rings the stones standing in the stream.
        water.obstacles = STREAM_ROCKS
            .iter()
            .map(|&(x, across, scale)| {
                let (position, _) = along(&sections, x, across);
                WaterObstacle {
                    position,
                    radius: 0.55 * scale,
                }
            })
            .collect();
        Self {
            water,
            sections,
            bridges,
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

/// Stones standing in the stream: the x where the creek's centreline
/// crosses, share of its half width across it (left positive) and scale:
/// one outside the west wall, two on the bend past the farm, one between
/// the bridges and one outside the east wall.
const STREAM_ROCKS: [(f32, f32, f32); 5] = [
    (-36.0, 0.45, 1.0),
    (-21.0, -0.5, 1.3),
    (-14.0, 0.55, 1.1),
    (9.0, -0.45, 1.0),
    (38.0, 0.4, 1.2),
];

/// The creek's section where its centreline crosses `x`: the creek runs
/// west to east without turning back, so each x has one.
fn at_x(sections: &[WaterSection], x: f32) -> WaterSection {
    *sections
        .iter()
        .min_by(|a, b| (a.center.x - x).abs().total_cmp(&(b.center.x - x).abs()))
        .unwrap()
}

/// The point where the creek crosses `x`, `across` its half width to the
/// left, and the flow's yaw there in degrees.
fn along(sections: &[WaterSection], x: f32, across: f32) -> (Vec2, f32) {
    let section = at_x(sections, x);
    (
        section.center + section.normal * section.half_width * across,
        section.tangent.x.atan2(section.tangent.y).to_degrees(),
    )
}

/// The ground kind of the one-metre cell at `x`, `z` in the farmland, or
/// `None` where the farmland leaves the cell as it was.
pub fn field_rows_along_x(x: i32, z: i32) -> Option<bool> {
    FIELDS
        .iter()
        .find(|field| field.rect.contains(x, z))
        .map(|field| field.rows_along_x)
}

/// The ground kind of the one-metre cell at `x`, `z` in the farmland, or
/// `None` where the farmland leaves the cell as it was: the fields.
pub fn farm_ground(x: i32, z: i32) -> Option<&'static str> {
    FIELDS
        .iter()
        .any(|field| field.rect.contains(x, z))
        .then_some("field")
}

impl Author {
    /// The farmland's pieces above the ground: the creek and its blockers
    /// and bridge, the farmhouse and water tower, hedgerows and the
    /// outside's fences and trees. The ground draws its fields, crop rows
    /// and the mud along the banks.
    pub fn farmland(&mut self, creek: &Creek) {
        self.creek(creek);
        for (&(name, _), section) in BRIDGES.iter().zip(&creek.bridges) {
            self.bridge(name, section);
        }
        self.farmhouse();
        self.prop_at("water_tower", "Arena Prop Farm Water Tower", TOWER, 0.0);
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
        // Inside the border, and just past it where the creek leaves through
        // its gaps on the west and the east.
        let in_pen = |p: Vec2| p.x.abs() < 30.2 && p.y.abs() < 35.0;
        // A section's blocker reaches this far either side of its centre.
        let reach = |section: &WaterSection| section.half_width + BLOCKER_MARGIN;
        let frames: Vec<BridgeFrame> = creek.bridges.iter().map(BridgeFrame::new).collect();
        // The rails' outer faces, across a bridge's deck.
        let rail = BRIDGE_HALF_WIDTH + RAIL_THICKNESS;
        let on_bridge = |section: &WaterSection| {
            frames.iter().any(|frame| {
                (section.s - frame.section.s).abs() < BRIDGE_LENGTH
                    && [-1.0, 1.0].iter().any(|side| {
                        let edge = section.center + section.normal * reach(section) * *side;
                        let (a, b) = (frame.local(section.center).x, frame.local(edge).x);
                        a.max(b) + 0.5 > -rail && a.min(b) - 0.5 < rail
                    })
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
        // Either side of each bridge, in its frame, a block closes the creek
        // from the last blocker to the rail, whatever angle the water
        // crosses the deck at.
        for (&(name, _), frame) in BRIDGES.iter().zip(&frames) {
            for (side, lo, hi) in [
                ("West", -rail - 2.4, -rail + RAIL_THICKNESS),
                ("East", rail - RAIL_THICKNESS, rail + 2.4),
            ] {
                let ends: Vec<f32> = creek
                    .sections
                    .iter()
                    .filter(|section| {
                        (lo - 0.6..=hi + 0.6).contains(&frame.local(section.center).x)
                            && (section.s - frame.section.s).abs() < 6.0
                    })
                    .flat_map(|section| {
                        [-1.0, 1.0].map(|s| {
                            frame
                                .local(section.center + section.normal * reach(section) * s)
                                .y
                        })
                    })
                    .collect();
                let z0 = ends.iter().copied().fold(f32::MAX, f32::min);
                let z1 = ends.iter().copied().fold(f32::MIN, f32::max);
                self.tank_blocker(
                    &format!("Arena Creek Blocker Bridge {name} {side}"),
                    frame.world(vec2((lo + hi) * 0.5, (z0 + z1) * 0.5)),
                    vec2(hi - lo, z1 - z0),
                    frame.yaw,
                );
            }
        }
        // Reeds along the banks, on the lip at the water's edge: where the
        // creek crosses x, on its north (1) or south (-1) bank.
        for (i, &(x, side)) in [
            (-27.0, 1.0),
            (-19.5, -1.0),
            (2.0, 1.0),
            (14.0, -1.0),
            (24.5, 1.0),
        ]
        .iter()
        .enumerate()
        {
            let section = at_x(&creek.sections, x);
            let at = section.center + section.normal * side * (section.half_width + 0.45);
            self.kit(
                "grass_tuft",
                &format!("Arena Grass Creek Reeds {i}"),
                vec3(at.x, BANK_TOP, at.y),
                (i * 53) as f32,
            );
        }
    }

    /// A bridge, `Arena Prop Bridge {name}`, square across the creek at
    /// `section`, and its rails, which stop tanks but let rounds and sight
    /// across.
    fn bridge(&mut self, name: &str, section: &WaterSection) {
        let frame = BridgeFrame::new(section);
        self.kit(
            "bridge",
            &format!("Arena Prop Bridge {name}"),
            vec3(frame.center.x, 0.0, frame.center.y),
            frame.yaw,
        );
        for (i, side) in [-1.0, 1.0].into_iter().enumerate() {
            self.tank_blocker(
                &format!("Arena Bridge Rail {name} {i}"),
                frame.world(vec2(side * (BRIDGE_HALF_WIDTH + RAIL_THICKNESS * 0.5), 0.0)),
                vec2(RAIL_THICKNESS, BRIDGE_LENGTH),
                frame.yaw,
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

    /// Outside the walls: fences along the fields, and trees in copses
    /// along the creek, at the fields' corners and off the corners.
    fn outside(&mut self, creek: &Creek) {
        for (i, (x, z, yaw)) in [
            (-33.0, -15.0, 90.0),
            (-33.0, -11.0, 90.0),
            (-33.0, -7.0, 90.0),
            (-34.0, -37.0, 0.0),
            (-30.0, -37.0, 0.0),
            (-26.0, -37.0, 0.0),
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
            ("tree_large", -37.5, -32.0),
            ("tree", -36.5, -29.0),
            ("tree", -40.5, -25.5),
            ("tree_large", -21.0, -37.5),
            ("tree", -19.0, -40.5),
            ("tree_small", -4.0, -37.5),
            ("tree_large", 17.0, -38.5),
            ("tree_large", 36.5, -29.0),
            ("tree", 36.5, 24.0),
            ("tree_small", 1.5, 37.5),
            ("tree_large", -27.0, 36.5),
            ("tree", -31.0, 39.5),
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
