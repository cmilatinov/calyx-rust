//! Rebuild the arena kit and explicit gameplay placements, preserving actor ids.
//! Run from the repository root: cargo run -p sandbox --example remake_arena.
#![recursion_limit = "256"]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ColliderShape, ComponentAmbientOcclusion, ComponentAntiAliasing, ComponentBloom,
    ComponentCollider, ComponentColorGrading, ComponentDirectionalLight, ComponentID,
    ComponentMesh, ComponentRigidBody, ComponentSkyLight, ComponentTonemap, ComponentTransform,
    ComponentVignette, FxaaQuality, TonemappingPreset,
};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{TypeUuid, Uuid};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use rapier3d::dynamics::RigidBodyType;
use sandbox::{
    arena::ComponentArena, tank_camera::ComponentTankCamera, terrain::ComponentDriveSurface,
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

    /// The walls inside the pen. Corners shield the spawn from the two sand
    /// lanes, a run across the west middle splits the lane from the centre,
    /// a corner in the northwest faces the raised area across the north,
    /// and a run in the east middle screens the west ramp's approach from
    /// the centre. Sight between these opens and closes as tanks move. Each
    /// leaves the sand lanes, the ramps' feet and the spawn's first six
    /// metres clear, and keeps a gap wider than a tank, at least 2.8 m, to
    /// other obstacles and 4.8 m to the perimeter, so no tank wedges beside
    /// one.
    fn interior_walls(&mut self) {
        let run = |from: (f32, f32), to: (f32, f32)| (vec2(from.0, from.1), vec2(to.0, to.1));
        // Corner posts sit at the inner angle; their arms reach 3 m along
        // local +X and +Z, and an end cap closes on its local -X.
        self.interior_wall(
            "Southwest Corner",
            &[
                ("wall_corner", -16.0, -25.0, 0.0),
                ("wall_long", -16.0, -20.0, 90.0),
                ("wall_end", -16.0, -17.0, 90.0),
                ("wall_end", -12.0, -25.0, 180.0),
            ],
            &[
                run((-16.0, -25.6), (-16.0, -16.4)),
                run((-16.6, -25.0), (-11.4, -25.0)),
            ],
        );
        self.interior_wall(
            "Southeast Corner",
            &[
                ("wall_corner", 16.0, -25.0, -90.0),
                ("wall_long", 16.0, -20.0, 90.0),
                ("wall_end", 16.0, -17.0, 90.0),
                ("wall_end", 12.0, -25.0, 0.0),
            ],
            &[
                run((16.0, -25.6), (16.0, -16.4)),
                run((11.4, -25.0), (16.6, -25.0)),
            ],
        );
        self.interior_wall(
            "West Middle",
            &[
                ("wall_end", -16.0, 11.0, 0.0),
                ("wall_long", -13.0, 11.0, 0.0),
                ("wall_end", -10.0, 11.0, 180.0),
            ],
            &[run((-16.6, 11.0), (-9.4, 11.0))],
        );
        self.interior_wall(
            "Northwest Corner",
            &[
                ("wall_corner", -6.0, 25.0, 180.0),
                ("wall_short", -6.0, 21.0, 90.0),
                ("wall_end", -6.0, 19.0, -90.0),
                ("wall_long", -11.0, 25.0, 0.0),
                ("wall_end", -14.0, 25.0, 0.0),
            ],
            &[
                run((-6.0, 18.4), (-6.0, 25.6)),
                run((-14.6, 25.0), (-5.4, 25.0)),
            ],
        );
        self.interior_wall(
            "East Middle",
            &[
                ("wall_end", 10.0, 11.0, -90.0),
                ("wall_long", 10.0, 14.0, 90.0),
                ("wall_end", 10.0, 17.0, 90.0),
            ],
            &[run((10.0, 10.4), (10.0, 17.6))],
        );
    }

    /// An interior wall, `Arena Wall {name}`: the kit's wall modules joined
    /// at their connection ports, and one continuous solid along each
    /// straight run, from `from` to `to` on its centre line, so a tank
    /// sliding along it meets no seam between modules. Like every wall solid
    /// it blocks sight and rises to the shell-blocking top.
    fn interior_wall(&mut self, name: &str, pieces: &[WallPiece], runs: &[(Vec2, Vec2)]) {
        let name = format!("Arena Wall {name}");
        let wall = self.object(&name, self.arena, Vec3::zeros());
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
        let path = self
            .assets
            .join(format!("generated/prefabs/sunny/{model}.cxprefab"));
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
        ("Target A", -6.0, 24.0),
        ("Target B", 6.0, 32.0),
        ("Sentry", 16.0, 32.0),
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
    let children: Vec<_> = scene.children(arena).collect();
    for child in children {
        scene.delete(child);
    }
    scene.flush_deletes();
    let mut author = Author {
        scene,
        arena,
        assets: root.join("assets"),
        remap: BTreeMap::new(),
        identity_bases: BTreeMap::new(),
        names: Default::default(),
    };

    // One uninterrupted collision floor keeps one-metre tile seams from
    // catching tracks. Every visible tile has its top at the same y = 0.
    author.drive_surface(
        arena,
        "Arena Floor",
        vec3(0.0, -0.5, 0.0),
        vec3(60.0, 1.0, 64.0),
        UnitQuaternion::identity(),
    );
    let foundation = author.object("Arena Foundation", arena, vec3(0.0, -0.6, 0.0));
    // Continue the visible ground beyond the pen, including the camera's
    // far zoom and aim lead at the walls. Its top stays below the tiles to
    // avoid z-fighting; collision remains confined to the playfield above.
    author.scene.set_transform(
        foundation,
        &Transform::from_components(
            vec3(0.0, -0.6, 0.0),
            Default::default(),
            vec3(200.0, 1.0, 200.0),
        )
        .matrix(),
    );
    author.scene.add_component(
        foundation,
        ComponentMesh {
            mesh: AssetRef::from_id("4d2d87eb-60b4-4dcc-b3d4-67bc444396c9".parse().unwrap()),
            material: AssetRef::from_id(asset_id(
                &author.assets.join("generated/materials/sunny/ground.cxmat"),
            )),
            ..Default::default()
        },
    );
    let ground_kind = |x: i32, z: i32| {
        if (-3..3).contains(&x) && (-26..-20).contains(&z) {
            "paving"
        } else if (-1..2).contains(&x) && (6..18).contains(&z) {
            if x == 0 {
                "road_marked"
            } else {
                "road"
            }
        } else if (((-22..-18).contains(&x) || (18..22).contains(&x)) && (-27..27).contains(&z))
            || ((-22..22).contains(&x) && (-5..-2).contains(&z))
        {
            "sand"
        } else if (x <= -24 || x >= 24) && (z <= -18 || z >= 20) {
            "dirt"
        } else if (-29..29).contains(&x) && (-31..31).contains(&z) {
            // Open grass inside the pen is the terrain's; see `terrain`.
            "terrain"
        } else {
            "grass"
        }
    };
    // Uniform, untextured tile surfaces can share one scaled instance per
    // rectangle. Keep paving joints and road dashes on individual metre cells.
    let mut covered = std::collections::BTreeSet::new();
    for x in -30..30 {
        for z in -32..32 {
            if covered.contains(&(x, z)) {
                continue;
            }
            let kind = ground_kind(x, z);
            let mut width = 1;
            let mut depth = 1;
            if kind != "paving" && kind != "road_marked" {
                while x + width < 30
                    && ground_kind(x + width, z) == kind
                    && !covered.contains(&(x + width, z))
                {
                    width += 1;
                }
                while z + depth < 32
                    && (x..x + width).all(|column| {
                        ground_kind(column, z + depth) == kind
                            && !covered.contains(&(column, z + depth))
                    })
                {
                    depth += 1;
                }
            }
            for column in x..x + width {
                for row in z..z + depth {
                    assert!(covered.insert((column, row)));
                }
            }
            if kind == "terrain" {
                continue;
            }
            let position = vec3(
                x as f32 + width as f32 * 0.5,
                -0.1,
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
    assert_eq!(covered.len(), 60 * 64);

    for (name, x, z, yaw) in [
        ("SW", -29.0, -31.0, 0.0),
        ("SE", 29.0, -31.0, -90.0),
        ("NW", -29.0, 31.0, 90.0),
        ("NE", 29.0, 31.0, 180.0),
    ] {
        author.kit(
            "wall_corner",
            &format!("Arena Wall {name}"),
            vec3(x, 0.0, z),
            yaw,
        );
    }
    for (side, z) in [("South", -31.0), ("North", 31.0)] {
        for x in (-24..=24).step_by(4) {
            author.kit(
                "wall_long",
                &format!("Arena Wall {side} {x}"),
                vec3(x as f32, 0.0, z),
                0.0,
            );
        }
    }
    for (side, x) in [("West", -29.0), ("East", 29.0)] {
        for z in (-26..=26).step_by(4) {
            author.kit(
                "wall_long",
                &format!("Arena Wall {side} {z}"),
                vec3(x, 0.0, z as f32),
                90.0,
            );
        }
    }

    // Straight continuous collision strips follow the modular silhouettes
    // without internal coplanar seams that can catch a sliding tank.
    for (name, center, size) in [
        ("West", vec3(-29.0, 0.8, 0.0), vec3(1.2, 1.6, 63.2)),
        ("East", vec3(29.0, 0.8, 0.0), vec3(1.2, 1.6, 63.2)),
        ("South", vec3(0.0, 0.8, -31.0), vec3(59.2, 1.6, 1.2)),
        ("North", vec3(0.0, 0.8, 31.0), vec3(59.2, 1.6, 1.2)),
    ] {
        let (center, size) = blocking_shells(center, size);
        author.collider(arena, &format!("Arena Wall {name} collision"), center, size);
    }

    // Three staggered clusters interrupt direct shots without sealing off
    // either flank; the spawn has a clear first six metres ahead of it.
    for (model, name, x, z, yaw) in [
        ("barrier_hazard", "Left Barrier", -6.0, -3.0, 0.0),
        ("barrier_plain", "Centre Barrier", 0.0, -1.0, 0.0),
        ("sandbag_stack", "Right Sandbags", 5.0, -2.0, 90.0),
        ("crate_large", "Left Crate", -7.0, 2.0, 0.0),
        ("crate_framed", "Left Supply", -5.0, 3.0, 0.0),
        ("crate_framed", "Right Supply", 5.0, 3.0, 0.0),
        ("barrier_hazard", "Rear Barrier", -5.0, 8.0, 0.0),
        ("crate_large", "Rear Crate", 0.0, 11.0, 0.0),
        ("rock_large", "West Boulder", -12.0, -3.0, 25.0),
        ("rock_small", "West Stone", -12.0, -1.0, 0.0),
        ("rock_medium", "East Rock", 12.0, 0.0, -20.0),
        ("rock_small", "East Stone", 12.0, 2.0, 0.0),
    ] {
        author.prop(model, &format!("Arena Cover {name}"), x * 2.0, z * 2.0, yaw);
    }
    // Companion cover makes clusters at the larger spacing, with clear
    // four-metre sand lanes at X +/-20 and a clear spawn approach.
    for (model, name, x, z, yaw) in [
        ("barrier_hazard", "Spawn Left Barrier", -7.0, -14.5, 0.0),
        ("crate_large", "Spawn Right Crate", 7.0, -14.0, 0.0),
        ("crate_framed", "Spawn Left Supply", -9.0, -14.0, 0.0),
        ("crate_framed", "Left Forward Crate", -14.0, -5.0, 0.0),
        ("barrier_plain", "Centre Wing Barrier", -4.0, -3.0, 20.0),
        ("crate_large", "Right Forward Crate", 12.0, -5.0, 0.0),
        ("sandbag_stack", "Left Rear Sandbags", -13.0, 17.0, 0.0),
        ("crate_framed", "Rear Supply", 3.0, 22.0, 0.0),
        ("crate_large", "West Middle Crate", -25.0, 7.0, 0.0),
        ("crate_framed", "East Middle Crate", 25.0, 8.0, 0.0),
    ] {
        author.prop(model, &format!("Arena Cover {name}"), x, z, yaw);
    }
    author.raised_area();
    author.interior_walls();
    for (model, name, x, z, yaw) in [
        ("tree_small", "Spawn West Tree", -6.0, -9.0, 20.0),
        ("tree_small", "Spawn East Tree", 6.0, -8.0, -20.0),
        ("tree", "Southwest Tree", -11.8, -12.5, 0.0),
        ("tree_small", "West Tree", -12.0, 5.0, 30.0),
        ("tree_large", "Northwest Tree", -11.0, 12.0, 0.0),
        ("tree", "Southeast Tree", 11.8, -12.5, 45.0),
        ("berry_bush", "Southwest Bush", -12.0, -8.0, 20.0),
        ("berry_bush", "Northwest Bush", -9.0, 13.0, 0.0),
        ("flag", "Spawn Flag", -3.5, -10.5, 0.0),
        ("lamp", "Spawn Lamp", 3.5, -10.5, 0.0),
    ] {
        author.prop(model, &format!("Arena Prop {name}"), x * 2.0, z * 2.0, yaw);
    }
    for (i, (x, z)) in [
        (-12.0, -13.0),
        (-10.0, -9.0),
        (12.0, -9.0),
        (10.0, -13.0),
        (-12.0, 9.0),
        (-8.0, 13.0),
        (-9.0, 4.0),
        (-4.0, 5.0),
        (6.0, -5.0),
        (3.0, 1.0),
        (-4.0, -5.0),
        (12.0, 5.0),
    ]
    .into_iter()
    .enumerate()
    {
        author.prop(
            "grass_tuft",
            &format!("Arena Grass {i}"),
            x * 2.0,
            z * 2.0,
            (i * 37) as f32,
        );
    }

    author.terrain(&["Player Spawn", "Target A", "Target B", "Sentry"]);

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
    std::fs::write(&path, output).unwrap();
    println!(
        "Authored {} objects into {}",
        data.components.len(),
        path.display()
    );
}

/// The terrain's samples: one every half metre over the 60 by 64 m pen,
/// the same grid the one-metre kit tiles sit on.
const RELIEF_COLUMNS: usize = 121;
const RELIEF_ROWS: usize = 129;
const RELIEF_SPACING: f32 = 0.5;
const RELIEF_SIZE: Vec2 = Vec2::new(60.0, 64.0);
/// How far below the floor the terrain lies under tiles, out of sight.
const RELIEF_BASE: f32 = 0.15;
/// The terrain's height range above its base: room for the highest hill.
const RELIEF_RANGE: f32 = 3.0;
/// The steepest the ground climbs to any of its eight neighbouring samples,
/// as rise over run, while it is shaped.
const RELIEF_GRADE: f32 = 0.335;
/// The steepest a pad's level may lie above the level ground round it, or
/// another pad, as rise over run between their edges.
const PAD_GRADE: f32 = 0.25;
/// The steepest any triangle of the surface may be: tanks drive every slope.
const RELIEF_MAX_SLOPE_DEGREES: f32 = 20.0;
/// Level ground kept at the floor round walls, the platform and ramps, and
/// round tiles; and at a prop's own level round its solids.
const RELIEF_WALL_MARGIN: f32 = 0.5;
/// Level ground round the platform and ramps, so tanks meet their sides
/// and feet on the floor.
const RELIEF_RAISED_AREA_MARGIN: f32 = 2.5;
const RELIEF_TILE_MARGIN: f32 = 0.0;
const RELIEF_PAD_MARGIN: f32 = 0.75;
/// Level ground round the spawn, targets and sentry.
const RELIEF_ACTOR_PAD: f32 = 2.0;
/// The colour map's texels per metre.
const RELIEF_COLOR_DENSITY: usize = 8;
/// The colour map's contour interval and line width, in metres.
const RELIEF_CONTOUR: f32 = 0.5;
const RELIEF_CONTOUR_WIDTH: f32 = 0.18;

/// A landform the relief is shaped from: ground rising to `height` along
/// the segment from `from` to `to`, out to `top` metres from it, then
/// falling away. The grade and the level ground round everything placed
/// cut it down where it would be too steep or too close.
struct Landform {
    from: Vec2,
    to: Vec2,
    top: f32,
    height: f32,
}

const fn landform(from: (f32, f32), to: (f32, f32), top: f32, height: f32) -> Landform {
    Landform {
        from: Vec2::new(from.0, from.1),
        to: Vec2::new(to.0, to.1),
        top,
        height,
    }
}

/// The arena's hills, ridges and berms, in its frame. Hills fill the open
/// ground between the cover clusters; the valleys between them, the road
/// north of the centre, the sentry's field of fire and the raised area stay
/// at the floor. Berms run along the outer side of both sand lanes.
const LANDFORMS: [Landform; 8] = [
    // South of the cross lane, either side of the spawn's approach.
    landform((-10.0, -12.0), (-6.0, -10.0), 1.5, 2.8),
    landform((6.0, -10.0), (10.0, -12.0), 1.5, 2.8),
    // West of the centre, between the west lane and Target A.
    landform((-14.0, 2.0), (-12.0, 7.0), 1.0, 2.8),
    // Between the west middle wall and the northwest corner.
    landform((-14.0, 15.5), (-10.0, 19.0), 1.0, 2.8),
    // North of the centre, west of the raised area's ramp.
    landform((-2.0, 25.0), (3.0, 26.0), 1.0, 2.8),
    // East, south of the cross lane.
    landform((13.0, -12.0), (13.0, -8.0), 1.0, 2.8),
    // Berms along the outer side of the sand lanes.
    landform((-25.2, -15.0), (-25.2, 16.0), 0.5, 2.0),
    landform((25.2, -15.0), (25.2, 16.0), 0.5, 2.0),
];

impl Author {
    /// The terrain under the open grass, `Arena Terrain`: hills, ridges and
    /// berms written to `game/textures/arena_heightmap.png`, coloured by the
    /// baked `game/textures/arena_terrain_color.png` through the
    /// `game/materials/arena_terrain` material. It replaces the grass tiles
    /// inside the pen; where other tiles lie it sinks below them.
    ///
    /// Walls, the platform, the ramps and their feet, the tiles and the
    /// `actors` keep level ground at the floor round them. Every prop with
    /// solids stands on a level pad at the height of the ground under it,
    /// so its solids still rise to the shell-blocking top above its own
    /// ground; grass tufts stand on the slopes. No triangle is steeper than
    /// [`RELIEF_MAX_SLOPE_DEGREES`]. Run after everything else is placed.
    fn terrain(&mut self, actors: &[&str]) {
        let arena = self.scene.world_transform(self.arena).position;
        let actors: Vec<_> = actors
            .iter()
            .map(|&name| {
                let object = self
                    .scene
                    .objects()
                    .find(|&o| self.scene.name(o) == name)
                    .unwrap();
                let position = self.scene.world_transform(object).position - arena;
                vec2(position.x, position.z)
            })
            .collect();
        let relief = self.relief(&actors);

        // The props stand on their pads.
        for (prop, height) in &relief.pads {
            let mut pose = self.scene.transform(*prop);
            pose.position.y = *height;
            self.scene.set_transform(*prop, &pose.matrix());
        }

        let range = RELIEF_RANGE;
        let level = |height: f32| ((height + RELIEF_BASE) / range).clamp(0.0, 1.0);
        let textures = self.assets.join("game/textures");
        std::fs::create_dir_all(&textures).unwrap();
        let heightmap_path = textures.join("arena_heightmap.png");
        image::ImageBuffer::<image::Luma<u16>, _>::from_fn(
            RELIEF_COLUMNS as u32,
            RELIEF_ROWS as u32,
            |column, row| {
                let height = relief.heights[row as usize * RELIEF_COLUMNS + column as usize];
                image::Luma([(level(height) * 65535.0).round() as u16])
            },
        )
        .save(&heightmap_path)
        .unwrap();
        let heightmap = stable("Arena Terrain heightmap");
        let texture_type = calyx_engine::assets::texture::Texture::type_uuid();
        write_meta(&heightmap_path, heightmap, "game/textures", texture_type);

        // The drawn surface, sampled as the game does.
        let field = calyx_engine::math::Heightfield::new(
            RELIEF_COLUMNS,
            RELIEF_ROWS,
            relief.heights.iter().map(|&height| level(height)).collect(),
        )
        .unwrap();
        let extent = calyx_engine::math::HeightfieldExtent {
            size: RELIEF_SIZE,
            height_scale: range,
        };
        let ground = |point: Vec2| {
            let half = RELIEF_SIZE * 0.5;
            let point = point.sup(&-half).inf(&half);
            field.sample(&extent, point).unwrap()
        };

        let color_path = textures.join("arena_terrain_color.png");
        relief_colors(|point| ground(point).height - RELIEF_BASE)
            .save(&color_path)
            .unwrap();
        let color = stable("Arena Terrain colour");
        write_meta(&color_path, color, "game/textures", texture_type);

        // The Sunny ground material, coloured by the map instead of the
        // flat grass.
        let materials = self.assets.join("game/materials");
        let material_path = materials.join("arena_terrain.cxmat");
        let mut material = read(&self.assets.join("generated/materials/sunny/ground.cxmat"));
        for variable in material["variables"].as_array_mut().unwrap() {
            match variable["name"].as_str().unwrap() {
                "base_color_texture" => {
                    variable["value"] = json!({ "Texture2D": { "Asset": color } })
                }
                "base_color_factor" => variable["value"] = json!({ "Color": [1.0, 1.0, 1.0, 1.0] }),
                _ => {}
            }
        }
        std::fs::write(
            &material_path,
            serde_json::to_string_pretty(&material).unwrap(),
        )
        .unwrap();
        let material = stable("Arena Terrain material");
        write_meta(
            &material_path,
            material,
            "game/materials",
            calyx_engine::assets::material::Material::type_uuid(),
        );

        let terrain = self.object("Arena Terrain", self.arena, vec3(0.0, -RELIEF_BASE, 0.0));
        self.scene.add_component(
            terrain,
            calyx_engine::component::ComponentTerrain {
                heightmap: AssetRef::from_id(heightmap),
                material: AssetRef::from_id(material),
                size: RELIEF_SIZE,
                height_scale: range,
                cast_shadows: true,
            },
        );

        // Tufts stand on the drawn surface.
        let tufts: Vec<_> = self
            .scene
            .children(self.arena)
            .filter(|&object| self.scene.name(object).starts_with("Arena Grass "))
            .collect();
        for tuft in tufts {
            let mut pose = self.scene.transform(tuft);
            let point = vec2(pose.position.x, pose.position.z);
            pose.position.y = ground(point).height - RELIEF_BASE;
            self.scene.set_transform(tuft, &pose.matrix());
        }
    }

    /// Shapes the relief: see [`Self::terrain`].
    fn relief(&self, actors: &[Vec2]) -> Relief {
        let to_arena = self.scene.world_transform(self.arena).inverse_matrix();
        // The arena-frame XZ bounds of an object's box collider.
        let bounds = |object: GameObject| {
            let half =
                self.scene
                    .read_component::<ComponentCollider, _, _>(object, |collider| match collider
                        .shape
                    {
                        ColliderShape::Cuboid { half_extents } => half_extents,
                        _ => panic!("arena solids are boxes"),
                    })?;
            let world = to_arena * self.scene.world_transform(object).matrix();
            let mut min = Vec2::repeat(f32::INFINITY);
            let mut max = Vec2::repeat(f32::NEG_INFINITY);
            for corner in 0..8 {
                let sign = |bit: usize| if corner & bit == 0 { -1.0 } else { 1.0 };
                let point = world
                    * nalgebra_glm::vec4(sign(1) * half.x, sign(2) * half.y, sign(4) * half.z, 1.0);
                min = min.inf(&vec2(point.x, point.z));
                max = max.sup(&vec2(point.x, point.z));
            }
            Some((min, max))
        };

        // Level ground at the floor: walls, the platform and ramps, the
        // ramps' feet, tiles and actors.
        let mut floor: Vec<(Vec2, Vec2, f32)> = Vec::new();
        // Props standing on the ground, with the bounds of their solids.
        let mut props: Vec<(GameObject, Vec2, Vec2)> = Vec::new();
        for child in self.scene.children(self.arena).collect::<Vec<_>>() {
            let name = self.scene.name(child);
            if name == "Arena Floor" || name.starts_with("Arena Grass ") {
                continue;
            }
            let solids: Vec<_> = std::iter::once(child)
                .chain(self.scene.descendants(child))
                .filter_map(bounds)
                .collect();
            if solids.is_empty() {
                continue;
            }
            let is_prop = name.starts_with("Arena Cover ") || name.starts_with("Arena Prop ");
            if is_prop && self.scene.transform(child).position.y == 0.0 {
                let min = solids
                    .iter()
                    .fold(Vec2::repeat(f32::INFINITY), |a, b| a.inf(&b.0));
                let max = solids
                    .iter()
                    .fold(Vec2::repeat(f32::NEG_INFINITY), |a, b| a.sup(&b.1));
                props.push((child, min, max));
            } else {
                let raised = name.starts_with("Arena Platform") || name.starts_with("Arena Ramp");
                let margin = if raised {
                    RELIEF_RAISED_AREA_MARGIN
                } else {
                    RELIEF_WALL_MARGIN
                };
                floor.extend(solids.into_iter().map(|(min, max)| (min, max, margin)));
            }
        }
        // Each ramp's foot: level ground three metres out from its low end.
        for name in ["Arena Ramp South", "Arena Ramp West"] {
            let ramp = self
                .scene
                .children(self.arena)
                .find(|&o| self.scene.name(o) == name)
                .unwrap();
            let pose = to_arena * self.scene.world_transform(ramp).matrix();
            let corners = [(-2.5, -4.0), (2.5, -4.0), (-2.5, -7.0), (2.5, -7.0)]
                .map(|(x, z)| pose * nalgebra_glm::vec4(x, 0.0, z, 1.0))
                .map(|p| vec2(p.x, p.z));
            let min = corners
                .iter()
                .fold(Vec2::repeat(f32::INFINITY), |a, b| a.inf(b));
            let max = corners
                .iter()
                .fold(Vec2::repeat(f32::NEG_INFINITY), |a, b| a.sup(b));
            floor.push((min, max, RELIEF_WALL_MARGIN));
        }
        // The one-metre cells kit tiles cover; the rest is open grass.
        let mut tiled = vec![false; 60 * 64];
        for object in self.scene.children(self.arena) {
            if !self.scene.name(object).starts_with("Arena Ground ") {
                continue;
            }
            let pose = self.scene.transform(object);
            let min = (pose.position.xz() - pose.scale.xz() * 0.5).map(|v| v.round() as i32);
            let max = (pose.position.xz() + pose.scale.xz() * 0.5).map(|v| v.round() as i32);
            for x in min.x..max.x {
                for z in min.y..max.y {
                    tiled[((x + 30) * 64 + z + 32) as usize] = true;
                }
            }
            floor.push((min.cast(), max.cast(), RELIEF_TILE_MARGIN));
        }
        for actor in actors {
            let reach = Vec2::repeat(RELIEF_ACTOR_PAD);
            floor.push((actor - reach, actor + reach, 0.0));
        }
        let is_tiled = |x: i32, z: i32| {
            !(-30..30).contains(&x)
                || !(-32..32).contains(&z)
                || tiled[((x + 30) * 64 + z + 32) as usize]
        };

        let point = |index: usize| {
            let (column, row) = (index % RELIEF_COLUMNS, index / RELIEF_COLUMNS);
            vec2(
                column as f32 * RELIEF_SPACING - RELIEF_SIZE.x * 0.5,
                RELIEF_SIZE.y * 0.5 - row as f32 * RELIEF_SPACING,
            )
        };
        let inside = |p: Vec2, (min, max, margin): (Vec2, Vec2, f32)| {
            p.x >= min.x - margin
                && p.x <= max.x + margin
                && p.y >= min.y - margin
                && p.y <= max.y + margin
        };
        let count = RELIEF_COLUMNS * RELIEF_ROWS;
        // Samples under tiles on every side sink out of sight; a sample on
        // any open cell's edge stays at the floor.
        let sunk: Vec<bool> = (0..count)
            .map(|index| {
                let p = point(index);
                let cells = |v: f32| (v - 0.25).floor() as i32..=(v + 0.25).floor() as i32;
                cells(p.x).all(|cx| cells(p.y).all(|cz| is_tiled(cx, cz)))
            })
            .collect();
        let mut fixed: Vec<Option<f32>> = (0..count)
            .map(|index| {
                let p = point(index);
                (sunk[index] || floor.iter().any(|&area| inside(p, area))).then_some(0.0)
            })
            .collect();
        // The landforms, with a little rolling noise over them.
        let mut heights: Vec<f32> = (0..count)
            .map(|index| {
                let p = point(index);
                let height = LANDFORMS
                    .iter()
                    .map(|form| {
                        let along = form.to - form.from;
                        let t =
                            ((p - form.from).dot(&along) / along.norm_squared()).clamp(0.0, 1.0);
                        let distance = (p - (form.from + along * t)).norm();
                        let run = form.height / RELIEF_GRADE * 1.6;
                        form.height * smoothstep(form.top + run, form.top, distance)
                    })
                    .fold(0.0, f32::max)
                    * (0.85 + 0.3 * rolling_noise(p));
                height.min(RELIEF_RANGE - RELIEF_BASE - 0.1)
            })
            .collect();
        let neighbours = |index: usize| {
            let (column, row) = (
                (index % RELIEF_COLUMNS) as i32,
                (index / RELIEF_COLUMNS) as i32,
            );
            [
                (-1, 0),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
            ]
            .into_iter()
            .filter_map(move |(dc, dr)| {
                let (c, r) = (column + dc, row + dr);
                ((0..RELIEF_COLUMNS as i32).contains(&c) && (0..RELIEF_ROWS as i32).contains(&r))
                    .then(|| {
                        let run = if dc != 0 && dr != 0 {
                            RELIEF_SPACING * std::f32::consts::SQRT_2
                        } else {
                            RELIEF_SPACING
                        };
                        (r as usize * RELIEF_COLUMNS + c as usize, run)
                    })
            })
        };
        // Grades the ground: lowers every free sample to within `grade` of
        // its neighbours or, with `raise`, lifts it to within `grade` below
        // them, until nothing moves.
        let grade = |heights: &mut Vec<f32>, fixed: &[Option<f32>], grade: f32, raise: bool| {
            for (index, value) in fixed.iter().enumerate() {
                if let Some(value) = value {
                    heights[index] = *value;
                }
            }
            for _ in 0..2000 {
                let mut moved = false;
                for pass in [false, true] {
                    let order: Box<dyn Iterator<Item = usize>> = if pass {
                        Box::new((0..count).rev())
                    } else {
                        Box::new(0..count)
                    };
                    for index in order {
                        if fixed[index].is_some() {
                            continue;
                        }
                        let mut value = heights[index];
                        for (next, run) in neighbours(index) {
                            value = if raise {
                                value.max(heights[next] - grade * run)
                            } else {
                                value.min(heights[next] + grade * run)
                            };
                        }
                        if (value - heights[index]).abs() > 1e-5 {
                            heights[index] = value;
                            moved = true;
                        }
                    }
                }
                if !moved {
                    return;
                }
            }
            panic!("the relief does not settle");
        };
        let smooth = |heights: &mut Vec<f32>, fixed: &[Option<f32>]| {
            let previous = heights.clone();
            for index in 0..count {
                if fixed[index].is_none() {
                    let (sum, n) = neighbours(index)
                        .fold((previous[index], 1.0), |(sum, n), (next, _)| {
                            (sum + previous[next], n + 1.0)
                        });
                    heights[index] = sum / n;
                }
            }
        };
        grade(&mut heights, &fixed, RELIEF_GRADE, false);
        for _ in 0..3 {
            smooth(&mut heights, &fixed);
        }

        // Pads: props whose solids lie close together share one, at the
        // lowest ground under any of their origins.
        let mut groups: Vec<(Vec<usize>, Vec2, Vec2)> = Vec::new();
        for (i, &(_, min, max)) in props.iter().enumerate() {
            let reach = Vec2::repeat(RELIEF_PAD_MARGIN * 2.0);
            let joined: Vec<usize> = (0..groups.len())
                .filter(|&g| {
                    let (_, gmin, gmax) = groups[g];
                    min.x - reach.x <= gmax.x
                        && gmin.x <= max.x + reach.x
                        && min.y - reach.y <= gmax.y
                        && gmin.y <= max.y + reach.y
                })
                .collect();
            let mut group = (vec![i], min, max);
            for &g in joined.iter().rev() {
                let (members, gmin, gmax) = groups.remove(g);
                group.0.extend(members);
                group.1 = group.1.inf(&gmin);
                group.2 = group.2.sup(&gmax);
            }
            groups.push(group);
        }
        // How high a pad may stand at each sample and still leave room for
        // a drivable slope down to the level ground at the floor.
        let mut room = vec![f32::INFINITY; count];
        grade(&mut room, &fixed, PAD_GRADE, false);
        let cells: Vec<Vec<usize>> = groups
            .iter()
            .map(|(members, ..)| {
                (0..count)
                    .filter(|&index| {
                        !sunk[index]
                            && members.iter().any(|&i| {
                                let (_, min, max) = props[i];
                                inside(point(index), (min, max, RELIEF_PAD_MARGIN))
                            })
                    })
                    .collect()
            })
            .collect();
        // Each pad lies at the mean ground under it, cut into the slope on
        // one side and built up on the other, within its room.
        let mut levels: Vec<f32> = cells
            .iter()
            .map(|cells| {
                let mean =
                    cells.iter().map(|&index| heights[index]).sum::<f32>() / cells.len() as f32;
                cells.iter().map(|&index| room[index]).fold(mean, f32::min)
            })
            .collect();
        // Neighbouring pads also leave room for a slope between them.
        let margin = Vec2::repeat(RELIEF_PAD_MARGIN);
        for _ in 0..groups.len() {
            for i in 0..groups.len() {
                for j in 0..groups.len() {
                    let (_, imin, imax) = groups[i];
                    let (_, jmin, jmax) = groups[j];
                    let gap = ((imin - margin) - (jmax + margin))
                        .sup(&((jmin - margin) - (imax + margin)))
                        .sup(&Vec2::zeros())
                        .norm();
                    levels[i] = levels[i].min(levels[j] + PAD_GRADE * gap);
                }
            }
        }
        let mut pads = Vec::new();
        for (g, (members, ..)) in groups.iter().enumerate() {
            // Down to the centimetre, so the authored height reads cleanly.
            let height = ((levels[g] * 100.0).floor() / 100.0).max(0.0);
            for &index in &cells[g] {
                fixed[index] = Some(height);
            }
            pads.extend(members.iter().map(|&i| (props[i].0, height)));
        }
        // Settle the ground round the pads, then keep every triangle within
        // the steepest slope a tank drives. The ground falls from each pad
        // no steeper than the pad grade, which keeps even the triangles of
        // that fall within the limit; elsewhere it is lowered, triangle by
        // triangle, until none is too steep.
        let limit = RELIEF_MAX_SLOPE_DEGREES.to_radians().tan() + 1e-4;
        let mut floor_under = vec![f32::NEG_INFINITY; count];
        grade(&mut floor_under, &fixed, PAD_GRADE, true);
        // Nor higher than the lower pads and level ground leave room for.
        let mut ceiling = vec![f32::INFINITY; count];
        grade(&mut ceiling, &fixed, PAD_GRADE, false);
        for index in 0..count {
            floor_under[index] = floor_under[index].min(ceiling[index]);
        }
        grade(&mut heights, &fixed, RELIEF_GRADE, false);
        // Round off the creases grading leaves, and roughen the slopes a
        // little so the contours wander as a hillside's do.
        for index in 0..count {
            if fixed[index].is_none() {
                let rise = smoothstep(0.0, 0.6, heights[index]);
                heights[index] +=
                    0.3 * (rolling_noise(point(index) * 1.7 + vec2(40.0, 40.0)) - 0.5) * rise;
            }
        }
        for _ in 0..2 {
            smooth(&mut heights, &fixed);
        }
        for index in 0..count {
            heights[index] = heights[index].max(floor_under[index]);
        }
        for _ in 0..5000 {
            let mut changed = false;
            for pairs in relief_triangles() {
                if pairs.iter().flatten().any(|&c| sunk[c]) {
                    continue;
                }
                for (i, corner) in pairs.into_iter().flatten().enumerate() {
                    // Each corner once: the pairs share one.
                    let shared = pairs.into_iter().flatten().take(i).any(|c| c == corner);
                    if shared || fixed[corner].is_some() {
                        continue;
                    }
                    let Some((_, high)) = corner_range(&heights, pairs, corner, limit * 0.999)
                    else {
                        continue;
                    };
                    let value = heights[corner].min(high).max(floor_under[corner]);
                    if value < heights[corner] - 1e-6 {
                        heights[corner] = value;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (index, value) in fixed.iter().enumerate() {
            if let Some(value) = value {
                assert!(
                    (heights[index] - value).abs() < 1e-4,
                    "level ground at {:?} could not be kept",
                    point(index)
                );
            }
        }
        let steepest = relief_triangles()
            .filter(|pairs| !pairs.iter().flatten().any(|&c| sunk[c]))
            .map(|pairs| triangle_slope(&heights, pairs))
            .fold(0.0, f32::max);
        assert!(
            steepest <= limit,
            "the relief is {:.1} degrees steep",
            steepest.atan().to_degrees()
        );
        for (index, height) in heights.iter_mut().enumerate() {
            if sunk[index] {
                *height = -RELIEF_BASE;
            }
        }
        Relief { heights, pads }
    }
}

/// The relief's heights above the floor, row by row from the +Z edge, and
/// each prop with the height of the level pad it stands on.
struct Relief {
    heights: Vec<f32>,
    pads: Vec<(GameObject, f32)>,
}

/// Every triangle of the relief's surface, split as
/// [`calyx_engine::math::Heightfield`] splits its cells, as the sample
/// pairs along a row (+X) and down a column (-Z) that set its slope.
fn relief_triangles() -> impl Iterator<Item = [[usize; 2]; 2]> {
    (0..RELIEF_ROWS - 1).flat_map(|row| {
        (0..RELIEF_COLUMNS - 1).flat_map(move |column| {
            let a = row * RELIEF_COLUMNS + column;
            let (b, c, d) = (a + 1, a + RELIEF_COLUMNS, a + RELIEF_COLUMNS + 1);
            // Triangles a, c, b and b, c, d.
            [[[a, b], [a, c]], [[c, d], [b, d]]]
        })
    })
}

/// The heights `corner` of a triangle from [`relief_triangles`] may take,
/// the others staying put, for the triangle to be no steeper than `limit`,
/// or the gentlest height when the others alone are steeper; `None` when
/// the corner is not the triangle's.
fn corner_range(
    heights: &[f32],
    [[x0, x1], [z0, z1]]: [[usize; 2]; 2],
    corner: usize,
    limit: f32,
) -> Option<(f32, f32)> {
    let s = RELIEF_SPACING;
    let h = |i: usize| if i == corner { 0.0 } else { heights[i] };
    let role = |a: usize, b: usize| ((corner == b) as i32 - (corner == a) as i32) as f32 / s;
    // The slope is g + e * height.
    let g = vec2((h(x1) - h(x0)) / s, (h(z1) - h(z0)) / s);
    let e = vec2(role(x0, x1), role(z0, z1));
    let (a, b, c) = (
        e.norm_squared(),
        2.0 * g.dot(&e),
        g.norm_squared() - limit * limit,
    );
    let discriminant = b * b - 4.0 * a * c;
    (a > 0.0).then(|| {
        // Where no height is gentle enough, the gentlest one.
        let root = discriminant.max(0.0).sqrt();
        ((-b - root) / (2.0 * a), (-b + root) / (2.0 * a))
    })
}

/// Rise over run of a triangle from [`relief_triangles`].
fn triangle_slope(heights: &[f32], [[x0, x1], [z0, z1]]: [[usize; 2]; 2]) -> f32 {
    let dx = (heights[x1] - heights[x0]) / RELIEF_SPACING;
    let dz = (heights[z1] - heights[z0]) / RELIEF_SPACING;
    dx.hypot(dz)
}

/// The terrain's colour, [`RELIEF_COLOR_DENSITY`] texels per metre, read
/// like a clay relief map: the Sunny grass on the floor, turning to a
/// lighter, sunnier grass as the ground rises, with a thin dark-leaf line
/// on every half-metre contour, so each hill shows its height and shape
/// from the high camera. `ground` is the drawn surface's height above the
/// floor.
fn relief_colors(ground: impl Fn(Vec2) -> f32) -> image::RgbImage {
    // Sunny Battlefield roles: ground grass, light leaf, dark leaf.
    let linear = |hex: u32| {
        Vec3::from_fn(|i, _| {
            let c = ((hex >> (16 - 8 * i)) & 0xff) as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    };
    let (grass, crest, line) = (linear(0x87BC6C), linear(0xAACF58), linear(0x5F8E2C));
    let width = (RELIEF_SIZE.x as usize) * RELIEF_COLOR_DENSITY;
    let depth = (RELIEF_SIZE.y as usize) * RELIEF_COLOR_DENSITY;
    let texel = 1.0 / RELIEF_COLOR_DENSITY as f32;
    image::RgbImage::from_fn(width as u32, depth as u32, |x, y| {
        let p = vec2(
            (x as f32 + 0.5) * texel - RELIEF_SIZE.x * 0.5,
            RELIEF_SIZE.y * 0.5 - (y as f32 + 0.5) * texel,
        );
        let h = ground(p).max(0.0);
        let step = 0.25;
        let (east, west) = (ground(p + vec2(step, 0.0)), ground(p - vec2(step, 0.0)));
        let (north, south) = (ground(p + vec2(0.0, step)), ground(p - vec2(0.0, step)));
        let slope = vec2(east - west, north - south).norm() / (2.0 * step);
        let mut color = grass.lerp(&crest, 0.8 * smoothstep(0.05, 1.8, h));
        // The nearest contour, as a distance across the ground.
        let level = (h / RELIEF_CONTOUR).round() * RELIEF_CONTOUR;
        if level > 0.0 && slope > 0.02 {
            let across = (h - level).abs() / slope;
            let ink = 1.0 - smoothstep(RELIEF_CONTOUR_WIDTH * 0.5, RELIEF_CONTOUR_WIDTH, across);
            color = color.lerp(&line, 0.5 * ink);
        }
        let srgb = color.map(|c| {
            let c = c.clamp(0.0, 1.0);
            let s = if c <= 0.0031308 {
                c * 12.92
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (s * 255.0).round() as u8
        });
        image::Rgb([srgb.x, srgb.y, srgb.z])
    })
}

/// Writes the `.meta` beside the asset at `path`, laid out as the asset
/// registry writes one, under the stable `id`.
fn write_meta(path: &Path, id: Uuid, folder: &str, type_uuid: Uuid) {
    let stem = path.file_stem().unwrap().to_str().unwrap();
    std::fs::write(
        path.with_extension("meta"),
        format!(
            "{{\n  \"main\": {{\n    \"id\": \"{id}\",\n    \"name\": \"{folder}/{stem}\",\n    \"display_name\": \"{stem}\",\n    \"type_uuid\": \"{type_uuid}\"\n  }},\n  \"inner\": []\n}}"
        ),
    )
    .unwrap();
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth value noise from 0 to 1 over the arena frame: two octaves on 7 m
/// and 3.5 m lattices whose corner values hash from their coordinates, so
/// the relief is the same on every run.
fn rolling_noise(point: Vec2) -> f32 {
    let lattice = |i: i32, j: i32, seed: u32| {
        let mut hash = (i as u32).wrapping_mul(0x8da6_b343)
            ^ (j as u32).wrapping_mul(0xd816_3841)
            ^ seed.wrapping_mul(0xcb1a_b31f);
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(0x5bd1_e995);
        hash ^= hash >> 15;
        (hash & 0xffff) as f32 / 65535.0
    };
    let octave = |spacing: f32, seed: u32| {
        let p = point / spacing;
        let (i, j) = (p.x.floor() as i32, p.y.floor() as i32);
        let fx = smoothstep(0.0, 1.0, p.x - i as f32);
        let fy = smoothstep(0.0, 1.0, p.y - j as f32);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        lerp(
            lerp(lattice(i, j, seed), lattice(i + 1, j, seed), fx),
            lerp(lattice(i, j + 1, seed), lattice(i + 1, j + 1, seed), fx),
            fy,
        )
    };
    (octave(7.0, 1) * 0.7 + octave(3.5, 2) * 0.3).clamp(0.0, 1.0)
}
