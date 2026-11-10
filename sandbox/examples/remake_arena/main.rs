//! Rebuild the arena kit and explicit gameplay placements, preserving actor ids.
//! Run from the repository root, in release (a debug build takes minutes to
//! paint the ground's masks): cargo run --release -p sandbox --example remake_arena.
//!
//! `-- --palette sunbaked|moonlit --out <scene>` writes the same arena with
//! that palette's kit, water and look-dev light rig to another scene, to
//! preview a palette; the arena itself is Sunny Battlefield's.
#![recursion_limit = "256"]

mod barn_yard;
mod border;
mod farmland;
mod ground;
mod layout;
mod orchard;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::component::{
    ColliderShape, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentCollider, ComponentColorGrading, ComponentDirectionalLight, ComponentID,
    ComponentRigidBody, ComponentSkyLight, ComponentTonemap, ComponentTransform, ComponentVignette,
    FxaaQuality, TonemappingPreset,
};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{TypeUuid, Uuid};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use rapier3d::dynamics::RigidBodyType;
use sandbox::{
    arena::ComponentArena, game_state::ComponentGameState, tank_camera::ComponentTankCamera,
    terrain::ComponentDriveSurface,
};
use serde_json::{json, Value};

fn stable(name: &str) -> Uuid {
    let hash = format!("calyx/arena/{name}").bytes().fold(
        0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128,
        |hash, byte| {
            (hash ^ u128::from(byte)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b)
        },
    );
    Uuid::from_u128(hash)
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn asset_id(path: &Path) -> Uuid {
    read(&path.with_extension("meta"))["main"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

// Keep actor JSON literals intact: serde_json's default float reader may
// round a double differently even when no gameplay value was edited.
fn component_text<'a>(text: &'a str, id: Uuid) -> &'a str {
    let start = text.find(&format!("    \"{id}\": {{")).unwrap();
    let end = start + text[start..].find("\n    }").unwrap() + "\n    }".len();
    &text[start..end]
}

struct Author {
    scene: Scene,
    /// The map whose kit the arena is built from.
    palette: calyx_model_gen::palette::Map,
    assets: PathBuf,
    arena: GameObject,
    remap: BTreeMap<String, String>,
    identity_bases: BTreeMap<Uuid, Value>,
    /// Every authored name, which seeds the object's stable id.
    names: std::collections::BTreeSet<String>,
}

impl Author {
    /// The raised area in the northeast pocket: a platform as high as the
    /// walls, reached by a ramp continuing the east sand lane from the south
    /// and a ramp from the middle of the arena on the west.
    fn raised_area(&mut self) {
        let platform = self.kit("platform", "Arena Platform", PLATFORM_CENTER, 0.0);
        // The paved top is a drive surface, so a ramp's crest never catches
        // the hull; the block below it stops tanks and rounds at its sides.
        self.drive_surface(
            platform,
            "Arena Platform deck",
            vec3(0.0, PLATFORM_HEIGHT - 0.2, 0.0),
            vec3(10.0, 0.4, 8.0),
            UnitQuaternion::identity(),
        );
        self.collider(
            platform,
            "Arena Platform body",
            vec3(0.0, (PLATFORM_HEIGHT - 0.4) * 0.5, 0.0),
            vec3(10.0, PLATFORM_HEIGHT - 0.4, 8.0),
        );
        // The rim kerbs, as drawn: low enough to shoot over, high enough to
        // keep tanks from driving off the edge.
        for (i, (x, z, width, depth)) in [
            (4.75, 0.0, 0.5, 8.0),
            (-4.75, -2.5, 0.5, 3.0),
            (-4.75, 3.5, 0.5, 1.0),
            (0.0, 3.75, 9.0, 0.5),
            (-3.25, -3.75, 2.5, 0.5),
            (3.25, -3.75, 2.5, 0.5),
        ]
        .into_iter()
        .enumerate()
        {
            self.collider(
                platform,
                &format!("Arena Platform kerb {i}"),
                vec3(x, PLATFORM_HEIGHT + KERB_HEIGHT * 0.5, z),
                vec3(width, KERB_HEIGHT, depth),
            );
        }
        for (name, x, z, yaw) in [("South", 20.0, 15.0, 0.0), ("West", 11.0, 24.0, 90.0)] {
            let name = format!("Arena Ramp {name}");
            let ramp = self.kit("ramp", &name, vec3(x, 0.0, z), yaw);
            self.ramp_solids(ramp, &name);
        }
        // Cover on the deck, overlooking the middle of the arena.
        self.prop_at(
            "sandbag_stack",
            "Arena Cover Platform Sandbags",
            vec3(17.0, PLATFORM_HEIGHT, 21.0),
            0.0,
        );
    }

    /// An interior wall, `Arena Wall {name}`: the kit's wall modules joined
    /// at their connection ports, and one continuous solid along each
    /// straight run, from `from` to `to` on its centre line, so a tank
    /// sliding along it meets no seam between modules. Like every wall solid
    /// it blocks sight and rises to the shell-blocking top. Its pieces and
    /// runs are in a frame at `origin` turned `yaw` degrees, so the wall may
    /// run at any angle to the arena's axes.
    fn interior_wall_at(
        &mut self,
        name: &str,
        origin: Vec2,
        yaw: f32,
        pieces: &[WallPiece],
        runs: &[(Vec2, Vec2)],
    ) {
        let name = format!("Arena Wall {name}");
        let position = vec3(origin.x, 0.0, origin.y);
        let wall = self.object(&name, self.arena, position);
        self.scene.set_transform(
            wall,
            &Transform::from_components(
                position,
                UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        for (i, &(model, x, z, yaw)) in pieces.iter().enumerate() {
            self.kit_under(wall, model, &format!("{name} {i}"), vec3(x, 0.0, z), yaw);
        }
        for (i, &(from, to)) in runs.iter().enumerate() {
            let along = to - from;
            assert!(
                along.x == 0.0 || along.y == 0.0,
                "{name} run {i} is not straight along an axis"
            );
            let center = (from + to) * 0.5;
            let size = vec3(
                along.x.abs() + if along.x == 0.0 { WALL_THICKNESS } else { 0.0 },
                WALL_HEIGHT,
                along.y.abs() + if along.y == 0.0 { WALL_THICKNESS } else { 0.0 },
            );
            let (center, size) = blocking_shells(vec3(center.x, WALL_HEIGHT * 0.5, center.y), size);
            self.collider(wall, &format!("{name} collision {i}"), center, size);
        }
    }

    /// The ramp's slope, as a tilted slab tanks drive on, and its kerbs, as
    /// tilted blocks reaching from their sloped tops into the ground, which
    /// close the ramp's sides to tanks and rounds.
    fn ramp_solids(&mut self, ramp: GameObject, name: &str) {
        let pitch = (PLATFORM_HEIGHT / RAMP_RUN).atan();
        let length = PLATFORM_HEIGHT.hypot(RAMP_RUN);
        let tilt = UnitQuaternion::from_euler_angles(-pitch, 0.0, 0.0);
        // The slab's top face runs from the foot at (0, 0, -4) to the crest
        // at (0, 1.6, 4); its centre sits half its thickness below that face.
        let normal = tilt * Vec3::y();
        let slab = 0.6;
        self.drive_surface(
            ramp,
            &format!("{name} surface"),
            vec3(0.0, PLATFORM_HEIGHT * 0.5, 0.0) - normal * slab * 0.5,
            vec3(RAMP_WIDTH, slab, length),
            tilt,
        );
        // Deep enough that the bottom of the crest end reaches the ground.
        let depth = 2.3;
        for (i, side) in [-1.0, 1.0].into_iter().enumerate() {
            let top = vec3(
                side * (RAMP_WIDTH + KERB_WIDTH) * 0.5,
                PLATFORM_HEIGHT * 0.5 + KERB_HEIGHT,
                0.0,
            );
            self.tilted_collider(
                ramp,
                &format!("{name} kerb {i}"),
                top - normal * depth * 0.5,
                vec3(KERB_WIDTH, depth, length),
                tilt,
            );
        }
    }

    /// Claims `name` for one authored object: its stable id derives from it.
    fn claim(&mut self, name: &str) {
        assert!(self.names.insert(name.into()), "{name} is authored twice");
    }

    fn object(&mut self, name: &str, parent: GameObject, position: Vec3) -> GameObject {
        self.claim(name);
        let object = self.scene.create(
            Some(ComponentID {
                id: stable(name),
                name: name.into(),
                ..Default::default()
            }),
            Some(parent),
        );
        self.scene
            .set_transform(object, &nalgebra_glm::translation(&position));
        object
    }

    fn kit(&mut self, model: &str, name: &str, position: Vec3, yaw: f32) -> GameObject {
        self.kit_under(self.arena, model, name, position, yaw)
    }

    /// A linked instance of the Sunny kit's `model` under `parent`, at
    /// `position` turned `yaw` degrees in `parent`'s frame.
    fn kit_under(
        &mut self,
        parent: GameObject,
        model: &str,
        name: &str,
        position: Vec3,
        yaw: f32,
    ) -> GameObject {
        self.claim(name);
        let path = self.assets.join(format!(
            "generated/prefabs/{}/{model}.cxprefab",
            self.palette.folder()
        ));
        let source = read(&path);
        let prefab = Prefab {
            data: serde_json::from_value(source["data"].clone()).unwrap(),
            root: source["root"].as_str().unwrap().parse().unwrap(),
        };
        let (object, dropped) = self
            .scene
            .instantiate_linked_prefab(asset_id(&path), &prefab, Some(parent))
            .unwrap();
        assert!(dropped.is_empty(), "{model} dropped components");
        let root_id = self.scene.uuid(object);
        for member in self.scene.objects() {
            let Some(link) = self.scene.prefab_link(member) else {
                continue;
            };
            if link.instance != root_id {
                continue;
            }
            let next = stable(&format!("{name}/{}", link.object));
            self.remap
                .insert(self.scene.uuid(member).to_string(), next.to_string());
            let mut identity =
                prefab.data.components[&link.object][&ComponentID::type_uuid()].clone();
            identity["id"] = json!(next);
            self.identity_bases.insert(next, identity);
        }
        self.scene
            .write_component::<ComponentID, _>(object, |id| id.name = name.into());
        self.scene.set_transform(
            object,
            &Transform::from_components(
                position,
                UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        object
    }

    fn collider(&mut self, parent: GameObject, name: &str, center: Vec3, size: Vec3) -> GameObject {
        self.tilted_collider(parent, name, center, size, UnitQuaternion::identity())
    }

    /// A box collider turned by `rotation` about its centre.
    fn tilted_collider(
        &mut self,
        parent: GameObject,
        name: &str,
        center: Vec3,
        size: Vec3,
        rotation: UnitQuaternion<f32>,
    ) -> GameObject {
        let object = self.object(name, parent, center);
        if rotation != UnitQuaternion::identity() {
            self.scene.set_transform(
                object,
                &Transform::from_components(center, rotation, vec3(1.0, 1.0, 1.0)).matrix(),
            );
        }
        self.scene.add_component(
            object,
            ComponentRigidBody {
                ty: RigidBodyType::Fixed,
                gravity_scale: 0.0,
                can_sleep: false,
                ..Default::default()
            },
        );
        self.scene.add_component(
            object,
            ComponentCollider {
                shape: ColliderShape::Cuboid {
                    half_extents: size * 0.5,
                },
                friction: 0.8,
                ..Default::default()
            },
        );
        object
    }

    /// A collider tanks drive on: they ride its top instead of colliding
    /// with it, and the cursor aims at it.
    fn drive_surface(
        &mut self,
        parent: GameObject,
        name: &str,
        center: Vec3,
        size: Vec3,
        rotation: UnitQuaternion<f32>,
    ) {
        let object = self.tilted_collider(parent, name, center, size, rotation);
        self.scene.add_component(object, ComponentDriveSurface {});
    }

    fn prop(&mut self, model: &str, name: &str, x: f32, z: f32, yaw: f32) {
        self.prop_at(model, name, vec3(x, 0.0, z), yaw);
    }

    /// A prop standing at `position`, which may be on a raised deck. Its
    /// solids live in its own frame, so they rise to the shell-blocking top
    /// above the ground it stands on.
    fn prop_at(&mut self, model: &str, name: &str, position: Vec3, yaw: f32) {
        let root = self.kit(model, name, position, yaw);
        let bounds = |y, w, h, d| (vec3(0.0, y, 0.0), vec3(w, h, d));
        let solids = match model {
            "wall_long" => vec![bounds(0.8, 4.0, 1.6, 1.2)],
            "wall_short" => vec![bounds(0.8, 2.0, 1.6, 1.2)],
            "wall_corner" => vec![
                (vec3(1.2, 0.8, 0.0), vec3(3.6, 1.6, 1.2)),
                (vec3(0.0, 0.8, 1.2), vec3(1.2, 1.6, 3.6)),
            ],
            // The low foot and tapered body have separate narrow solids;
            // lifting loops and paint are decorative, as in the kit handoff.
            "barrier_hazard" | "barrier_plain" => vec![
                bounds(0.08, 2.55, 0.16, 1.12),
                bounds(0.25, 2.4, 0.26, 0.98),
                bounds(0.52, 2.4, 0.28, 0.66),
                bounds(0.85, 2.4, 0.38, 0.40),
            ],
            "crate_framed" => vec![bounds(0.55, 1.1, 1.1, 1.1)],
            "crate_large" => vec![bounds(0.825, 1.65, 1.65, 1.65)],
            "sandbag_stack" => vec![
                bounds(0.23925, 1.94, 0.4785, 0.43),
                (vec3(0.0, 0.63525, 0.03), vec3(1.32, 0.4785, 0.43)),
                bounds(1.03125, 0.7, 0.4785, 0.43),
            ],
            "rock_small" | "rock_medium" | "rock_large" => rock_solids(
                &self
                    .assets
                    .join(format!("generated/models/{model}__body.obj")),
            ),
            "tree" | "tree_small" | "tree_large" => vec![bounds(0.75, 0.6, 1.5, 0.6)],
            "flag" | "lamp" => vec![bounds(0.75, 0.32, 1.5, 0.32)],
            "berry_bush" => vec![bounds(0.25, 0.8, 0.5, 0.7)],
            "grass_tuft" => vec![],
            // The hedge's dense body; its outermost leaves are decorative.
            "hedgerow" => vec![bounds(0.8, 4.4, 1.6, 1.3)],
            // The tower's legs and footings, as one block: rounds and tanks
            // stop at it, and its tank and roof stand high above both.
            "water_tower" => vec![bounds(
                1.0,
                farmland::TOWER_SOLID,
                2.0,
                farmland::TOWER_SOLID,
            )],
            "hay_bale" => vec![bounds(0.62, 1.24, 1.24, 1.24)],
            _ => panic!("no collision handoff for {model}"),
        };
        for (i, (center, size)) in solids.into_iter().enumerate() {
            let (center, size) = blocking_shells(center, size);
            self.collider(root, &format!("{name} collision {i}"), center, size);
        }
    }
}

/// Top of every obstacle collider above the ground the obstacle stands on.
/// Rounds fly at the gun's height, about 1.6 m with a 0.18 m radius, above
/// the ground under the shooter and under the aim point, so cover drawn
/// lower than that would let them pass over it.
const SHELL_BLOCKING_TOP: f32 = 2.0;

/// Stretches a solid upward to [`SHELL_BLOCKING_TOP`] in its prop's frame,
/// keeping its bottom and footprint, so rounds stop at the obstacle instead
/// of flying over it. The frame's origin is the ground the prop stands on,
/// so cover on a raised deck rises above the deck, not above the floor.
/// The raised area's platform: centred in the arena's frame, as high as the
/// perimeter walls, with 0.5 m kerbs round its rim and along its ramps.
const PLATFORM_CENTER: Vec3 = Vec3::new(20.0, 0.0, 23.0);
const PLATFORM_HEIGHT: f32 = 1.6;
const KERB_HEIGHT: f32 = 0.5;
const KERB_WIDTH: f32 = 0.5;
/// A ramp climbs the platform's height over this run, a 1:5 slope, between
/// kerbs either side of a surface this wide.
const RAMP_RUN: f32 = 8.0;
const RAMP_WIDTH: f32 = 4.0;
/// The kit's wall modules: 1.6 m high and 1.2 m thick, on the 1 m grid.
const WALL_HEIGHT: f32 = 1.6;
const WALL_THICKNESS: f32 = 1.2;

/// A wall module of an interior wall: the kit model, its root's `x` and `z`
/// in the arena's frame and its yaw in degrees.
type WallPiece = (&'static str, f32, f32, f32);

fn blocking_shells(center: Vec3, size: Vec3) -> (Vec3, Vec3) {
    let bottom = center.y - size.y * 0.5;
    let top = (center.y + size.y * 0.5).max(SHELL_BLOCKING_TOP);
    (
        vec3(center.x, (bottom + top) * 0.5, center.z),
        vec3(size.x, top - bottom, size.z),
    )
}

// Clip the exported triangles to horizontal bands, keeping the rock's
// asymmetric shoulder and crown. These are conservative primitive slabs,
// not exact convex colliders; thin crown bands avoid a broad invisible cap.
fn rock_solids(path: &Path) -> Vec<(Vec3, Vec3)> {
    let mesh =
        calyx_model_gen::obj::ObjMesh::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (low, high) = mesh.bounds();
    let mut bottom = low[1];
    let mut solids = Vec::new();
    while bottom < high[1] - 1e-6 {
        let top = (bottom + if bottom >= high[1] - 0.3 { 0.05 } else { 0.2 }).min(high[1]);
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut include = |point: [f64; 3]| {
            for axis in 0..3 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        };
        for triangle in &mesh.triangles {
            let points = triangle.map(|corner| mesh.positions[corner.0]);
            for point in points {
                if (bottom..=top).contains(&point[1]) {
                    include(point);
                }
            }
            for edge in 0..3 {
                let a = points[edge];
                let b = points[(edge + 1) % 3];
                for plane in [bottom, top] {
                    if (a[1] < plane && b[1] > plane) || (b[1] < plane && a[1] > plane) {
                        let t = (plane - a[1]) / (b[1] - a[1]);
                        include(std::array::from_fn(|axis| {
                            a[axis] + t * (b[axis] - a[axis])
                        }));
                    }
                }
            }
        }
        let center = Vec3::from_fn(|axis, _| ((min[axis] + max[axis]) * 0.5) as f32);
        let size = Vec3::from_fn(|axis, _| (max[axis] - min[axis]) as f32);
        assert!(size.iter().all(|value| value.is_finite() && *value > 0.0));
        solids.push((center, size));
        bottom = top;
    }
    assert!(
        solids.len() <= 16,
        "revisit the collision budget for {}",
        path.display()
    );
    solids
}

fn remap(value: &mut Value, ids: &BTreeMap<String, String>) {
    match value {
        Value::String(s) => {
            if let Some(next) = ids.get(s) {
                *s = next.clone();
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|v| remap(v, ids)),
        Value::Object(object) => {
            let old = std::mem::take(object);
            for (key, mut v) in old {
                remap(&mut v, ids);
                object.insert(ids.get(&key).cloned().unwrap_or(key), v);
            }
        }
        _ => {}
    }
}

/// Light and post-process the arena as Sunny Battlefield, the map its kit is
/// coloured for: the sun, sky and global volume take the direction's
/// lighting and post-processing targets
/// (`docs/design/art/tank-game-direction.md`). Sunny Battlefield has no
/// haze, so the volume holds no fog; the sky's HDRI is the ambient and
/// image-based fill light, with no separate ambient light. The volume's ink
/// outlines and fog of war stay as authored. Returns the ids of the objects
/// it rewrote.
fn light_and_post_process(scene: &mut Scene) -> [Uuid; 3] {
    let find =
        |scene: &Scene, name: &str| scene.objects().find(|o| scene.name(*o) == name).unwrap();
    let sun = find(scene, "Sun");
    // Pitched 50 degrees down, then turned 30 degrees about the vertical.
    let rotation = UnitQuaternion::from_axis_angle(&Vec3::y_axis(), 30f32.to_radians())
        * UnitQuaternion::from_axis_angle(&Vec3::x_axis(), 50f32.to_radians());
    scene.write_component::<ComponentTransform, _>(sun, |component| {
        component.transform.rotation = rotation;
    });
    scene.add_component(
        sun,
        ComponentDirectionalLight {
            color: Color32::from_rgb(0xFF, 0xE3, 0xC2),
            intensity: 4.8,
            cast_shadows: true,
            // Covers the Far framing's deepest view of the ground.
            shadow_distance: 70.0,
            depth_bias: 1.0,
            normal_bias: 1.0,
        },
    );
    let sky = find(scene, "Sky");
    scene.write_component::<ComponentSkyLight, _>(sky, |sky| sky.intensity = 1.0);
    let volume = find(scene, "Post Process");
    scene.add_component(
        volume,
        ComponentTonemap {
            preset: TonemappingPreset::Aces,
            exposure_ev: 0.0,
        },
    );
    scene.add_component(
        volume,
        ComponentAmbientOcclusion {
            radius: 0.9,
            intensity: 2.2,
            bias: 0.025,
            sample_count: 16,
        },
    );
    scene.add_component(
        volume,
        ComponentBloom {
            threshold: 1.2,
            knee: 0.5,
            intensity: 0.15,
            radius: 6,
        },
    );
    scene.add_component(
        volume,
        ComponentColorGrading {
            contrast: 1.05,
            saturation: 1.2,
            temperature: 0.06,
            tint: 0.0,
        },
    );
    scene.add_component(
        volume,
        ComponentVignette {
            intensity: 0.12,
            smoothness: 0.45,
            color: Color32::BLACK,
        },
    );
    // High finds the dark, low-contrast edges fog of war leaves where a side
    // cannot see, and follows the walls' long, nearly level edges further.
    scene.add_component(
        volume,
        ComponentAntiAliasing {
            edge_threshold: 0.166,
            subpixel: 0.75,
            quality: FxaaQuality::High,
        },
    );
    [sun, sky, volume].map(|object| scene.uuid(object))
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("assets/game/scenes/arena.cxscene");
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .map(|i| args[i + 1].clone())
    };
    use calyx_model_gen::palette::Map;
    let palette = match arg("--palette").as_deref() {
        None | Some("sunny") => Map::SunnyBattlefield,
        Some("sunbaked") => Map::Sunbaked,
        Some("moonlit") => Map::Moonlit,
        Some(other) => panic!("no palette {other}"),
    };
    let out = arg("--out").map_or_else(|| path.clone(), PathBuf::from);
    // No asset scanning is needed to author references and prefab links.
    let assets = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    let original_text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    let original: SceneData = serde_json::from_str(&original_text).unwrap();
    let (mut scene, dropped) = Scene::load(&assets.registries.lock_read(), original.clone());
    assert!(dropped.is_empty(), "arena lost components");
    let arena = scene.objects().find(|o| scene.name(*o) == "Arena").unwrap();
    let old_layout: std::collections::BTreeSet<_> = scene
        .descendants(arena)
        .map(|object| scene.uuid(object))
        .collect();
    let arena_id = scene.uuid(arena);
    scene.write_component::<ComponentArena, _>(arena, |arena| {
        arena.generate_on_start = false;
        arena.columns = 30;
        arena.rows = 32;
    });
    let mut edited = std::collections::BTreeSet::from([arena_id]);
    // Absolute placements make regeneration idempotent. Only these actor
    // transforms change; model scales, ids and gameplay settings survive.
    for (name, x, z) in [
        ("Arena", 0.0, 22.0),
        ("Target A", -8.0, 34.5),
        ("Target B", 28.0, 23.0),
        ("Sentry", 20.0, 23.5),
    ] {
        let object = scene.objects().find(|o| scene.name(*o) == name).unwrap();
        let mut pose = scene.transform(object);
        pose.position.x = x;
        pose.position.z = z;
        scene.set_transform(object, &pose.matrix());
        edited.insert(scene.uuid(object));
    }
    let camera = scene
        .objects()
        .find(|o| scene.name(*o) == "Camera")
        .unwrap();
    let pose = scene
        .write_component::<ComponentTankCamera, _>(camera, |rig| {
            rig.zoom = 1.0;
            rig.camera_transform(Vec3::zeros(), 0.0, rig.zoom)
        })
        .unwrap();
    scene.write_component::<ComponentTransform, _>(camera, |component| {
        component.transform.position = pose.position;
    });
    edited.insert(scene.uuid(camera));
    edited.extend(light_and_post_process(&mut scene));
    // The match flow: the arena opens on the main menu, and the Game State
    // takes it through the lobby, the match and the end screen. Once
    // authored it is preserved like any other actor.
    if !scene.objects().any(|o| scene.name(o) == "Game State") {
        let state = scene.create(
            Some(ComponentID {
                id: stable("Game State"),
                name: "Game State".into(),
                ..Default::default()
            }),
            None,
        );
        scene.add_component(state, ComponentGameState::default());
    }
    // The north team's spawn, on the far bank from the Player Spawn's, which
    // the player's side (team 0, and tanks of no team) spawns at.
    let existing = scene.objects().find(|o| scene.name(*o) == "North Spawn");
    let north_spawn = existing.unwrap_or_else(|| {
        let spawn = scene.create(
            Some(ComponentID {
                id: stable("North Spawn"),
                name: "North Spawn".into(),
                ..Default::default()
            }),
            None,
        );
        scene.add_component(
            spawn,
            sandbox::spawn::ComponentSpawnPoint {
                team: 1,
                ..Default::default()
            },
        );
        spawn
    });
    scene.set_transform(
        north_spawn,
        &Transform::from_components(
            vec3(layout::NORTH_SPAWN.x, 0.0, layout::NORTH_SPAWN.y + 22.0),
            UnitQuaternion::from_euler_angles(0.0, std::f32::consts::PI, 0.0),
            vec3(1.0, 1.0, 1.0),
        )
        .matrix(),
    );
    edited.insert(scene.uuid(north_spawn));
    let children: Vec<_> = scene.children(arena).collect();
    for child in children {
        scene.delete(child);
    }
    scene.flush_deletes();
    let mut author = Author {
        scene,
        palette,
        arena,
        assets: root.join("assets"),
        remap: BTreeMap::new(),
        identity_bases: BTreeMap::new(),
        names: Default::default(),
    };

    let creek = farmland::Creek::new(&author.assets, palette);
    // The Arena's extent is the border's bounding rectangle, in whole
    // two-metre cells, which the minimap fits and the camera checks cover.
    let (low, high) = border::bounds();
    let half = vec2(low.x.abs().max(high.x), low.y.abs().max(high.y));
    let (columns, rows) = ((half.x).ceil() as u32, (half.y).ceil() as u32);
    author
        .scene
        .write_component::<ComponentArena, _>(arena, |arena| {
            arena.columns = columns;
            arena.rows = rows;
        });
    // One uninterrupted collision floor at the ground's level, y = 0, under
    // the whole border: the ground is drawn by one terrain (`ground.rs`),
    // which tanks never touch.
    author.drive_surface(
        arena,
        "Arena Floor",
        vec3(0.0, -0.5, 0.0),
        vec3(columns as f32 * 2.0, 1.0, rows as f32 * 2.0),
        UnitQuaternion::identity(),
    );
    let lanes: Vec<_> = layout::LANES
        .iter()
        .map(|lane| layout::lane_curve(lane))
        .collect();
    // The spawn's paving and the sand lanes; the ground paints the rest.
    let pen_kind = |x: i32, z: i32| {
        if (-3..3).contains(&x) && (-26..-20).contains(&z) {
            "paving"
        } else if layout::lane_distance(&lanes, vec2(x as f32 + 0.5, z as f32 + 0.5))
            < layout::LANE_HALF_WIDTH
        {
            "sand"
        } else {
            "grass"
        }
    };
    // The ground kind of each one-metre cell of the apron, the pen inside
    // it, or none where the creek's trench takes the cell.
    let ground_kind = |x: i32, z: i32| -> Option<&'static str> {
        if creek.carves(x, z) {
            return None;
        }
        let inside = border::inside(vec2(x as f32 + 0.5, z as f32 + 0.5));
        Some(farmland::farm_ground(x, z).unwrap_or_else(|| {
            if inside {
                pen_kind(x, z)
            } else {
                "grass"
            }
        }))
    };
    // The spawn's paving and the road stay tiles, raised 3 cm off the ground
    // so neither lies coplanar with it. A plain road shares one scaled
    // instance per rectangle; paving joints and road dashes keep one tile
    // per metre cell.
    const APRON: i32 = 100;
    let side = (2 * APRON) as usize;
    let cell = |x: i32, z: i32| (z + APRON) as usize * side + (x + APRON) as usize;
    let kinds: Vec<Option<&str>> = (0..side * side)
        .map(|i| ground_kind((i % side) as i32 - APRON, (i / side) as i32 - APRON))
        .collect();
    let mut covered = vec![false; side * side];
    for z in -APRON..APRON {
        for x in -APRON..APRON {
            let Some(kind) = kinds[cell(x, z)] else {
                continue;
            };
            // The ground terrain draws every other kind.
            if !matches!(kind, "paving" | "road" | "road_marked") {
                continue;
            }
            if covered[cell(x, z)] {
                continue;
            }
            let mut width = 1;
            let mut depth = 1;
            if kind != "paving" && kind != "road_marked" {
                while x + width < APRON
                    && kinds[cell(x + width, z)] == Some(kind)
                    && !covered[cell(x + width, z)]
                {
                    width += 1;
                }
                while z + depth < APRON
                    && (x..x + width).all(|column| {
                        kinds[cell(column, z + depth)] == Some(kind)
                            && !covered[cell(column, z + depth)]
                    })
                {
                    depth += 1;
                }
            }
            for column in x..x + width {
                for row in z..z + depth {
                    covered[cell(column, row)] = true;
                }
            }
            let position = vec3(
                x as f32 + width as f32 * 0.5,
                -0.07,
                z as f32 + depth as f32 * 0.5,
            );
            let tile = author.kit(
                &format!("ground_{kind}"),
                &format!("Arena Ground {x} {z}"),
                position,
                0.0,
            );
            author.scene.set_transform(
                tile,
                &Transform::from_components(
                    position,
                    Default::default(),
                    vec3(width as f32, 1.0, depth as f32),
                )
                .matrix(),
            );
        }
    }
    author.ground(&creek, &ground_kind);

    author.border(&creek);
    author.raised_area();
    author.layout();
    author.barn_yard(&creek);
    author.farmland(&creek);
    // The spawns' flags and lamps: the south's either side of its paving,
    // the north's by its gravel pad.
    for (model, name, x, z, yaw) in [
        ("flag", "Spawn Flag", -5.0, -27.5, 0.0),
        ("lamp", "Spawn Lamp", 5.0, -25.5, 0.0),
        ("flag", "North Spawn Flag", -15.5, 20.5, 0.0),
        ("lamp", "North Spawn Lamp", -6.5, 26.5, 0.0),
    ] {
        author.prop(model, &format!("Arena Prop {name}"), x, z, yaw);
    }
    // Tufts scattered over the open grass, unevenly.
    for (i, (x, z)) in [
        (-15.0, -1.5),
        (-13.5, 1.0),
        (5.5, -24.5),
        (17.0, -10.5),
        (15.5, -6.0),
        (-4.0, 6.0),
        (8.0, 21.5),
        (24.0, 20.5),
        (-25.0, 10.0),
        (-9.5, 15.5),
    ]
    .into_iter()
    .enumerate()
    {
        // On dry ground inside the border: no blades over the water.
        assert!(
            creek.beyond(vec2(x, z)) > 1.0 && border::beyond(vec2(x, z)) < -2.0,
            "tuft {i} at ({x}, {z}) is in the creek or past the border"
        );
        author.prop(
            "grass_tuft",
            &format!("Arena Grass {i}"),
            x,
            z,
            (i * 37) as f32,
        );
    }

    // The north-west corner's orchard and windmill, checked against every
    // solid placed above.
    author.orchard(&creek);

    // Preserve actors outside the explicit placement/camera edits; kit instances receive
    // deterministic ids, so rerunning this authoring recipe is a no-op.
    let mut value = serde_json::to_value(SceneData::from(&author.scene)).unwrap();
    remap(&mut value, &author.remap);
    let mut data: SceneData = serde_json::from_value(value).unwrap();
    let mut preserved = Vec::new();
    for (id, components) in original.components {
        if !edited.contains(&id) && !old_layout.contains(&id) {
            preserved.push((id, component_text(&original_text, id).to_string()));
            data.components.insert(id, components);
        }
    }
    for (id, identity) in author.identity_bases {
        let hash = identity
            .to_string()
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        data.prefab_links
            .get_mut(&id)
            .unwrap()
            .base
            .insert(ComponentID::type_uuid(), hash);
    }
    let mut output = serde_json::to_string_pretty(&data).unwrap();
    for (id, original) in preserved {
        let generated = component_text(&output, id).to_string();
        output = output.replacen(&generated, &original, 1);
    }
    if palette != Map::SunnyBattlefield {
        output = with_look_dev_rig(&root, &output, palette);
    }
    std::fs::write(&out, output).unwrap();
    println!(
        "Authored {} objects into {}",
        data.components.len(),
        out.display()
    );
}

/// `scene` (the arena's JSON) with its Sun, Sky and Post Process taking
/// `palette`'s look-dev rig from `lookdev/tank_direction.cxscene`, its
/// components enabled, under the arena's ids and names.
fn with_look_dev_rig(root: &Path, scene: &str, palette: calyx_model_gen::palette::Map) -> String {
    use calyx_model_gen::palette::Map;
    let look_dev = read(&root.join("assets/lookdev/tank_direction.cxscene"));
    let mut scene: Value = serde_json::from_str(scene).unwrap();
    let id_type = ComponentID::type_uuid().to_string();
    let name = |data: &Value, id: &str| {
        data["components"][id][&id_type]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    let group = match palette {
        Map::SunnyBattlefield => "Sunny Battlefield",
        Map::Sunbaked => "Sunbaked",
        Map::Moonlit => "Moonlit",
    };
    let children = |data: &Value, id: &str| -> Vec<String> {
        data["hierarchy"][id]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .map(|id| id.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let group = look_dev["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .find(|id| name(&look_dev, id) == group)
        .unwrap();
    let rig = children(&look_dev, &group)
        .into_iter()
        .find(|id| name(&look_dev, id) == "Rig")
        .unwrap();
    let ids: Vec<String> = scene["components"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for (target, sources) in [
        ("Sun", &["Sun", "Moon"][..]),
        ("Sky", &["Sky"][..]),
        ("Post Process", &["Post Process"][..]),
    ] {
        let source = children(&look_dev, &rig)
            .into_iter()
            .find(|id| sources.contains(&name(&look_dev, id).as_str()))
            .unwrap();
        let target = ids
            .iter()
            .find(|id| name(&scene, id) == target)
            .unwrap()
            .clone();
        let mut components = look_dev["components"][&source].clone();
        components[&id_type] = scene["components"][&target][&id_type].clone();
        scene["components"][&target] = components;
        // The look-dev scene disables every rig but Sunny Battlefield's; the
        // preview's rig is the one it lights with, so all of it is enabled.
        if let Some(Value::Object(map)) = scene
            .as_object_mut()
            .unwrap()
            .get_mut("disabled_components")
        {
            map.remove(&target);
        }
    }
    serde_json::to_string_pretty(&scene).unwrap()
}
