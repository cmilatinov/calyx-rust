//! Rebuild the arena kit and explicit gameplay placements, preserving actor ids.
//! Run from the repository root: cargo run -p sandbox --example remake_arena.
#![recursion_limit = "256"]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ColliderShape, ComponentCollider, ComponentID, ComponentMesh, ComponentRigidBody,
    ComponentTransform,
};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{TypeUuid, Uuid};
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

/// The gentle relief under the open ground. Its highest rise stays well
/// under the 2 m cover: a round flies at the gun's height above the ground
/// under the shooter and the aim point, so standing on a rise lifts it over
/// cover on lower ground by at most the rise. With 0.3 m, plus the barrel
/// tip's lift on the gentlest slope, the bottom of a round still passes no
/// higher than about 1.95 m over cover standing on level ground.
const RELIEF_HEIGHT: f32 = 0.3;
/// How far below the tiles the terrain lies where tiles cover the ground,
/// hidden under the lanes, paving and the outer ring.
const RELIEF_BASE: f32 = 0.15;
/// Ground the relief keeps level around cover, walls, ramps, the platform,
/// tiles and the actors, before it rises over this falloff.
const RELIEF_FLAT_MARGIN: f32 = 1.2;
const RELIEF_FALLOFF: f32 = 3.0;
/// The terrain's samples: one every half metre over the 60 by 64 m pen,
/// the same grid the one-metre kit tiles sit on.
const RELIEF_COLUMNS: usize = 121;
const RELIEF_ROWS: usize = 129;
const RELIEF_SIZE: Vec2 = Vec2::new(60.0, 64.0);

impl Author {
    /// The terrain under the open grass, `Arena Terrain`: a heightmap of
    /// gentle mounds, written to `game/textures/arena_heightmap.png`, drawn
    /// in the ground's grass material. It replaces the grass tiles inside
    /// the pen; where other tiles lie it sinks below them. It stays level
    /// under and around every solid and actor, so cover, walls, ramps,
    /// spawns, targets and the sentry stand on level ground as before, and
    /// the grass tufts are lifted onto it. `actors` names the objects that
    /// keep level ground around them. Run after everything else is placed.
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
        let heights = self.relief_heights(&actors);
        let path = self.assets.join("game/textures/arena_heightmap.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let range = RELIEF_HEIGHT + RELIEF_BASE;
        image::ImageBuffer::<image::Luma<u16>, _>::from_fn(
            RELIEF_COLUMNS as u32,
            RELIEF_ROWS as u32,
            |column, row| {
                let height = heights[row as usize * RELIEF_COLUMNS + column as usize];
                let level = ((height + RELIEF_BASE) / range).clamp(0.0, 1.0);
                image::Luma([(level * 65535.0).round() as u16])
            },
        )
        .save(&path)
        .unwrap();
        let heightmap = stable("Arena Terrain heightmap");
        // Laid out as the asset registry writes a texture's meta.
        std::fs::write(
            path.with_extension("meta"),
            format!(
                "{{
  \"main\": {{
    \"id\": \"{heightmap}\",
    \"name\": \"game/textures/arena_heightmap\",
    \"display_name\": \"arena_heightmap\",
    \"type_uuid\": \"{}\"
  }},
  \"inner\": []
}}",
                calyx_engine::assets::texture::Texture::type_uuid()
            ),
        )
        .unwrap();

        let terrain = self.object("Arena Terrain", self.arena, vec3(0.0, -RELIEF_BASE, 0.0));
        let material = asset_id(&self.assets.join("generated/materials/sunny/ground.cxmat"));
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

        // Tufts stand on the drawn surface: sample it as the game does.
        let field = calyx_engine::math::Heightfield::new(
            RELIEF_COLUMNS,
            RELIEF_ROWS,
            heights
                .iter()
                .map(|height| (height + RELIEF_BASE) / range)
                .collect(),
        )
        .unwrap();
        let extent = calyx_engine::math::HeightfieldExtent {
            size: RELIEF_SIZE,
            height_scale: range,
        };
        let tufts: Vec<_> = self
            .scene
            .children(self.arena)
            .filter(|&object| self.scene.name(object).starts_with("Arena Grass "))
            .collect();
        for tuft in tufts {
            let mut pose = self.scene.transform(tuft);
            let point = vec2(pose.position.x, pose.position.z);
            pose.position.y = field.sample(&extent, point).unwrap().height - RELIEF_BASE;
            self.scene.set_transform(tuft, &pose.matrix());
        }
    }

    /// The relief's height above the floor at every sample, row by row from
    /// the +Z edge: level (0) within [`RELIEF_FLAT_MARGIN`] of a solid, a
    /// tile or an actor, sunk to [`RELIEF_BASE`] below the floor under
    /// tiles, and rising to [`RELIEF_HEIGHT`] in mounds over the open grass.
    fn relief_heights(&self, actors: &[Vec2]) -> Vec<f32> {
        // The arena-frame XZ footprints of every solid but the floor.
        let arena = self.scene.world_transform(self.arena).inverse_matrix();
        let mut flat: Vec<(Vec2, Vec2)> = Vec::new();
        for object in self.scene.descendants(self.arena) {
            if self.scene.name(object) == "Arena Floor" {
                continue;
            }
            let Some(half) =
                self.scene
                    .read_component::<ComponentCollider, _, _>(object, |collider| {
                        match collider.shape {
                            ColliderShape::Cuboid { half_extents } => half_extents,
                            _ => panic!("arena solids are boxes"),
                        }
                    })
            else {
                continue;
            };
            let world = arena * self.scene.world_transform(object).matrix();
            let (mut min, mut max) = (Vec2::repeat(f32::INFINITY), Vec2::repeat(f32::NEG_INFINITY));
            for corner in 0..8 {
                let sign = |bit: usize| if corner & bit == 0 { -1.0 } else { 1.0 };
                let point = world
                    * nalgebra_glm::vec4(sign(1) * half.x, sign(2) * half.y, sign(4) * half.z, 1.0);
                min = min.inf(&vec2(point.x, point.z));
                max = max.sup(&vec2(point.x, point.z));
            }
            flat.push((min, max));
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
            flat.push((min.cast(), max.cast()));
        }
        let is_tiled = |x: i32, z: i32| {
            !(-30..30).contains(&x)
                || !(-32..32).contains(&z)
                || tiled[((x + 30) * 64 + z + 32) as usize]
        };
        let mut heights = Vec::with_capacity(RELIEF_COLUMNS * RELIEF_ROWS);
        for row in 0..RELIEF_ROWS {
            for column in 0..RELIEF_COLUMNS {
                let x = column as f32 * 0.5 - 30.0;
                let z = 32.0 - row as f32 * 0.5;
                // Under tiles on every side, the terrain sinks out of sight;
                // a sample on any open cell's edge stays at the floor.
                let cells = |v: f32| {
                    let low = (v - 0.25).floor() as i32;
                    let high = (v + 0.25).floor() as i32;
                    low..=high
                };
                if cells(x).all(|cx| cells(z).all(|cz| is_tiled(cx, cz))) {
                    heights.push(-RELIEF_BASE);
                    continue;
                }
                let point = vec2(x, z);
                let clearance = flat
                    .iter()
                    .map(|&(min, max)| {
                        let outside = (min - point).sup(&(point - max)).sup(&Vec2::zeros());
                        outside.magnitude()
                    })
                    .chain(actors.iter().map(|actor| (actor - point).magnitude() - 2.0))
                    .fold(f32::INFINITY, f32::min);
                let open = smoothstep(
                    RELIEF_FLAT_MARGIN,
                    RELIEF_FLAT_MARGIN + RELIEF_FALLOFF,
                    clearance,
                );
                let mounds = smoothstep(0.45, 0.85, rolling_noise(point));
                heights.push(RELIEF_HEIGHT * open * mounds);
            }
        }
        heights
    }
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
