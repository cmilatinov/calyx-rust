//! The farmland: a sunken creek that comes in under the west wall, bends
//! deep into the south half past the farm, and forks in the middle of the
//! pen: its north branch runs north to leave through the north border, its
//! south branch south-east to leave through the south border, so the creek
//! splits the pen into three: the north-west, the south-west and the east.
//! Three bridges at uneven positions cross it, one over each stretch; the
//! ruined farmhouse, a water tower and a field of crop rows stand in the
//! southwest; fields, fences and trees lie outside the border.
//!
//! Every position is in the Arena's frame, where the south spawn is at
//! (0, -22).

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
/// downstream, and its width at each point: 6 to 8 m. Inside the pen it
/// bends deep into the south half past the farm, forks in the middle, and
/// as its north branch winds north to leave through the north border;
/// outside, it keeps meandering to the apron's edge.
const CREEK: [(f32, f32, f32); 21] = [
    (-95.0, 13.0, 8.0),
    (-74.0, 8.0, 7.5),
    (-55.0, 11.0, 8.0),
    (-40.0, 6.5, 7.0),
    (-31.0, 1.5, 6.5),
    (-23.5, -4.5, 6.5),
    (-17.5, -8.0, 7.0),
    (-11.5, -5.5, 6.5),
    (-6.0, -2.0, 6.0),
    (0.0, 0.0, 6.5),
    (5.5, 2.5, 6.5),
    (6.0, 7.5, 6.0),
    (3.0, 12.0, 6.0),
    (0.0, 16.5, 6.5),
    (-1.0, 22.0, 6.0),
    (1.5, 28.0, 6.5),
    (3.5, 34.0, 7.0),
    (3.0, 45.0, 7.5),
    (9.0, 60.0, 8.0),
    (5.0, 75.0, 7.5),
    (9.0, 95.0, 8.0),
];

/// The south branch, from the fork, where it starts inside the creek's
/// water, south-east past the south spawn's bank to leave through the
/// south border, a little narrower than the creek, and on outside to the
/// apron's edge. It is no mirror of the north branch: it leaves the fork
/// at a shallower angle and bends east as it goes.
const BRANCH: [(f32, f32, f32); 11] = [
    (4.5, 1.5, 5.5),
    (8.5, -2.5, 5.5),
    (11.0, -8.0, 6.0),
    (11.0, -14.5, 6.0),
    (12.5, -21.0, 6.5),
    (15.0, -27.0, 6.0),
    (17.5, -34.0, 6.5),
    (16.0, -45.0, 7.0),
    (22.0, -60.0, 7.5),
    (18.0, -75.0, 7.0),
    (22.0, -95.0, 7.5),
];

/// The bridges, `(name, water, near)`: each stands on the water (0 the
/// creek and its north branch, 1 the south branch) where its centreline
/// passes nearest `near`, its deck square across it. West crosses the
/// creek's diagonal run before the fork, joining the two western banks;
/// North crosses the north branch high up, by the platform's west ramp;
/// South crosses the south branch on the east lane, by the south spawn.
pub const BRIDGES: [(&str, usize, (f32, f32)); 3] = [
    ("West", 0, (-9.0, -4.2)),
    ("North", 0, (-0.8, 23.5)),
    ("South", 1, (11.0, -15.5)),
];
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

const FIELDS: [Field; 7] = [
    // The farm's field, west of the water tower.
    Field {
        rect: Rect::new(-30, -21, -25, -14),
        rows_along_x: false,
    },
    // Outside the border: west, southwest, south, north, east and
    // northeast.
    Field {
        rect: Rect::new(-52, -14, -42, -2),
        rows_along_x: false,
    },
    Field {
        rect: Rect::new(-46, -52, -30, -44),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(-6, -54, 10, -46),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(-30, 44, -14, 52),
        rows_along_x: true,
    },
    Field {
        rect: Rect::new(46, -22, 56, -8),
        rows_along_x: false,
    },
    Field {
        rect: Rect::new(38, 31, 48, 41),
        rows_along_x: false,
    },
];

/// How far the farmhouse stands from where #566 built it: moved south
/// and west, back from the creek's bank, so tanks leave the west bridge
/// clear of its corner.
pub const FARMHOUSE: Vec2 = Vec2::new(-1.0, -2.5);

/// The water tower, in the farm's lobe, well inside the border: east of
/// the field and south of the farm's wall.
pub const TOWER: Vec3 = Vec3::new(-22.0, 0.0, -24.0);
/// The water tower's solid: its legs and footings, rising to the
/// shell-blocking top; its tank and roof stand high above it.
pub const TOWER_SOLID: f32 = 3.0;

/// The creek's water and cross-sections, its south branch's, and the
/// bridges over them.
pub struct Creek {
    /// The creek, then its south branch.
    pub waters: Vec<ComponentWater>,
    sections: Vec<Vec<WaterSection>>,
    /// Each bridge's water and the section under its middle, in
    /// [`BRIDGES`] order.
    pub bridges: Vec<(usize, WaterSection)>,
}

/// How far `p` lies beyond the shore of the water through `sections`:
/// negative over it.
fn beyond_sections(sections: &[WaterSection], p: Vec2) -> f32 {
    let mut beyond = f32::MAX;
    for pair in sections.windows(2) {
        let (a, b) = (pair[0].center, pair[1].center);
        let ab = b - a;
        let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
        let half = pair[0].half_width + (pair[1].half_width - pair[0].half_width) * f;
        beyond = beyond.min((p - (a + ab * f)).norm() - half);
    }
    beyond
}

/// The section of `sections` whose centre lies nearest `p`.
fn nearest(sections: &[WaterSection], p: Vec2) -> WaterSection {
    *sections
        .iter()
        .min_by(|a, b| (a.center - p).norm().total_cmp(&(b.center - p).norm()))
        .unwrap()
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
        let water = |points: &[(f32, f32, f32)]| ComponentWater {
            points: points
                .iter()
                .map(|&(x, z, width)| WaterPoint {
                    position: vec2(x, z),
                    width,
                })
                .collect(),
            cell_size: 0.8,
            shading: WaterShading::Smooth,
            deep_color,
            shallow_color,
            foam_color,
            glint_strength,
            bank_material: bank_material.clone(),
            bank_height: BANK_TOP - WATER_Y,
            bank_depth: BANK_DEPTH,
            bank_run: BANK_RUN,
            bank_lip: BANK_LIP,
            ..Default::default()
        };
        let mut waters = vec![water(&CREEK), water(&BRANCH)];
        let sections: Vec<Vec<WaterSection>> = waters
            .iter()
            .map(|water| water.path().sections().expect("the creek meshes"))
            .collect();
        // Where the branch joins the creek, each opens into the other: a
        // shore lying in the other's water draws no bank and no shore.
        for (i, water) in waters.iter_mut().enumerate() {
            let other = &sections[1 - i];
            for section in &sections[i] {
                for side in [-1.0, 1.0] {
                    let shore = section.center + section.normal * section.half_width * side;
                    if beyond_sections(other, shore) < -0.3 {
                        water.openings.push(WaterObstacle {
                            position: shore,
                            radius: 1.0,
                        });
                    }
                }
            }
            assert!(!water.openings.is_empty(), "the creek forks");
        }
        let bridges = BRIDGES
            .iter()
            .map(|&(_, water, (x, z))| (water, nearest(&sections[water], vec2(x, z))))
            .collect();
        // Foam rings the stones standing in the stream.
        for (i, water) in waters.iter_mut().enumerate() {
            water.obstacles = STREAM_ROCKS
                .iter()
                .filter(|rock| rock.0 == i)
                .map(|&(_, near, across, scale)| {
                    let (position, _) = along(&sections[i], near, across);
                    WaterObstacle {
                        position,
                        radius: 0.55 * scale,
                    }
                })
                .collect();
        }
        Self {
            waters,
            sections,
            bridges,
        }
    }

    /// How far `p` lies beyond the creek's nearest shore, its branch's
    /// included: negative over the water.
    pub fn beyond(&self, p: Vec2) -> f32 {
        self.sections
            .iter()
            .map(|sections| beyond_sections(sections, p))
            .fold(f32::MAX, f32::min)
    }

    /// Whether the one-metre ground cell at `x`, `z` reaches into the
    /// trench, so it is left out of the ground.
    pub fn carves(&self, x: i32, z: i32) -> bool {
        self.beyond(vec2(x as f32 + 0.5, z as f32 + 0.5)) < TRENCH + HALF_DIAGONAL
    }
}

/// Stones standing in the stream: their water, the point the centreline
/// passes nearest, share of its half width across it (left positive) and
/// scale: one outside the west border, two on the bend past the farm, one
/// in the north branch, one in the south branch and one in each branch
/// outside the border.
const STREAM_ROCKS: [(usize, (f32, f32), f32, f32); 7] = [
    (0, (-36.0, 3.5), 0.45, 1.0),
    (0, (-21.0, -6.5), -0.5, 1.3),
    (0, (-14.0, -7.0), 0.55, 1.1),
    (0, (4.0, 10.5), -0.45, 1.0),
    (1, (11.5, -19.0), 0.4, 1.05),
    (0, (3.0, 41.0), 0.4, 1.2),
    (1, (16.5, -41.0), -0.4, 1.1),
];

/// The point where the water through `sections` passes nearest `near`,
/// `across` its half width to the left, and the flow's yaw there in
/// degrees.
fn along(sections: &[WaterSection], near: (f32, f32), across: f32) -> (Vec2, f32) {
    let section = nearest(sections, vec2(near.0, near.1));
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
        for (&(name, _, _), (_, section)) in BRIDGES.iter().zip(&creek.bridges) {
            self.bridge(name, section);
        }
        self.farmhouse();
        self.prop_at("water_tower", "Arena Prop Farm Water Tower", TOWER, 0.0);
        // Round bales against the farmhouse's east wall.
        for (i, z) in [-12.0, -10.76].into_iter().enumerate() {
            self.prop(
                "hay_bale",
                &format!("Arena Cover Farm Hay Bale {i}"),
                -6.8 + FARMHOUSE.x,
                z + FARMHOUSE.y,
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
        for (i, water) in creek.waters.iter().enumerate() {
            let name = ["Arena Creek", "Arena Creek South Branch"][i];
            let object = self.object(name, self.arena, vec3(0.0, WATER_Y, 0.0));
            self.scene.add_component(object, water.clone());
        }
        for (i, &(water, near, across, scale)) in STREAM_ROCKS.iter().enumerate() {
            let (at, yaw) = along(&creek.sections[water], near, across);
            let rock = self.kit(
                "rock_medium",
                &format!("Arena Creek Stone {i}"),
                vec3(at.x, WATER_Y - 0.55 * scale, at.y),
                yaw + 25.0 * i as f32,
            );
            self.scale(rock, scale);
        }
        // Inside the border, and just past it where the creek and its
        // branches leave through their gaps.
        let in_pen = |p: Vec2| super::border::beyond(p) < 2.5;
        // A section's blocker reaches this far either side of its centre.
        let reach = |section: &WaterSection| section.half_width + BLOCKER_MARGIN;
        let frames: Vec<(usize, BridgeFrame)> = creek
            .bridges
            .iter()
            .map(|(water, section)| (*water, BridgeFrame::new(section)))
            .collect();
        // The rails' outer faces, across a bridge's deck.
        let rail = BRIDGE_HALF_WIDTH + RAIL_THICKNESS;
        let mut count = 0;
        for (water, sections) in creek.sections.iter().enumerate() {
            let on_bridge = |section: &WaterSection| {
                frames.iter().any(|(on, frame)| {
                    *on == water
                        && (section.s - frame.section.s).abs() < BRIDGE_LENGTH
                        && [-1.0, 1.0].iter().any(|side| {
                            let edge = section.center + section.normal * reach(section) * *side;
                            let (a, b) = (frame.local(section.center).x, frame.local(edge).x);
                            a.max(b) + 0.5 > -rail && a.min(b) - 0.5 < rail
                        })
                })
            };
            let mut chunks: Vec<Vec<WaterSection>> = vec![Vec::new()];
            for section in sections {
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
            for chunk in chunks.iter().filter(|chunk| chunk.len() >= 2) {
                let block = block(chunk);
                self.tank_blocker(
                    &format!("Arena Creek Blocker {count}"),
                    block.center,
                    block.size,
                    block.yaw,
                );
                count += 1;
            }
        }
        // Either side of each bridge, in its frame, a block closes the creek
        // from the last blocker to the rail, whatever angle the water
        // crosses the deck at.
        for (&(name, _, _), (water, frame)) in BRIDGES.iter().zip(&frames) {
            for (side, lo, hi) in [
                ("West", -rail - 2.4, -rail + RAIL_THICKNESS),
                ("East", rail - RAIL_THICKNESS, rail + 2.4),
            ] {
                let ends: Vec<f32> = creek.sections[*water]
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
        // Reeds along the banks, on the lip at the water's edge: on the
        // water, where its centreline passes nearest the point, on its left
        // (1) or right (-1) bank.
        for (i, &(water, near, side)) in [
            (0, (-27.0, -1.5), 1.0),
            (0, (-19.5, -7.5), -1.0),
            (0, (1.5, 14.5), 1.0),
            (1, (11.0, -11.0), -1.0),
            (0, (1.0, 26.0), -1.0),
            (1, (13.5, -24.0), 1.0),
        ]
        .iter()
        .enumerate()
        {
            let section = nearest(&creek.sections[water], vec2(near.0, near.1));
            let at = section.center + section.normal * side * (section.half_width + 0.45);
            assert!(creek.beyond(at) > 0.3, "reeds {i} stand in the other water");
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
        self.interior_wall_at(
            "Farmhouse",
            FARMHOUSE,
            0.0,
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
            vec3(-11.0 + FARMHOUSE.x, 0.0, -13.0 + FARMHOUSE.y),
            -12.0,
        );
    }

    /// Outside the border: fences along the fields, and trees in copses
    /// round it, each well clear of the border and the creek.
    fn outside(&mut self, creek: &Creek) {
        for (i, (x, z, yaw)) in [
            (-41.0, -12.0, 90.0),
            (-41.0, -8.0, 90.0),
            (-41.0, -4.0, 90.0),
            (-44.0, -43.0, 0.0),
            (-40.0, -43.0, 0.0),
            (-36.0, -43.0, 0.0),
            (-3.0, -45.0, 0.0),
            (1.0, -45.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                super::border::beyond(vec2(x, z)) > 2.5,
                "outside fence {i} stands inside the border"
            );
            self.kit(
                "fence",
                &format!("Arena Prop Outside Fence {i}"),
                vec3(x, 0.0, z),
                yaw,
            );
        }
        // Copses just outside the border all round, so its bends read
        // against trees from the gameplay camera, not bare ground.
        let trees = [
            ("tree_large", -42.0, -26.0),
            ("tree", -41.5, -31.0),
            ("tree_large", -45.0, -28.5),
            ("tree", -30.0, -44.5),
            ("tree_large", -20.0, -41.0),
            ("tree_small", -6.0, -43.5),
            ("tree_large", 28.0, -40.0),
            ("tree", 38.5, -35.0),
            ("tree_large", 39.0, -18.0),
            ("tree", 47.0, 0.0),
            ("tree_large", 41.0, 15.0),
            ("tree", 38.0, 27.0),
            ("tree_small", 28.0, 38.0),
            ("tree_large", 16.0, 43.5),
            ("tree", -12.0, 45.0),
            ("tree_large", -24.0, 38.5),
            ("tree", -42.0, 30.0),
            ("tree_large", -44.0, 20.0),
            ("tree", -38.0, -8.0),
        ];
        for (i, (model, x, z)) in trees.into_iter().enumerate() {
            assert!(
                creek.beyond(vec2(x, z)) > 2.0,
                "outside tree {i} stands in the creek"
            );
            assert!(
                super::border::beyond(vec2(x, z)) > 2.5,
                "outside tree {i} stands inside the border"
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
