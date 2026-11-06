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
use nalgebra_glm::{vec3, Vec3};
use rapier3d::dynamics::RigidBodyType;
use sandbox::{arena::ComponentArena, tank_camera::ComponentTankCamera};
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
}

impl Author {
    fn object(&mut self, name: &str, parent: GameObject, position: Vec3) -> GameObject {
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
            .instantiate_linked_prefab(asset_id(&path), &prefab, Some(self.arena))
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

    fn collider(&mut self, parent: GameObject, name: &str, center: Vec3, size: Vec3) {
        let object = self.object(name, parent, center);
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
    }

    fn prop(&mut self, model: &str, name: &str, x: f32, z: f32, yaw: f32) {
        let root = self.kit(model, name, vec3(x, 0.0, z), yaw);
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

/// Top of every obstacle collider. Rounds fly level at the gun's height,
/// about 1.6 m with a 0.18 m radius, so cover drawn lower than that would
/// let them pass over it.
const SHELL_BLOCKING_TOP: f32 = 2.0;

/// Stretches a solid upward to [`SHELL_BLOCKING_TOP`], keeping its bottom
/// and footprint, so rounds stop at the obstacle instead of flying over it.
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
        ("Campfire", -10.0, 6.0),
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
    };

    // One uninterrupted collision floor keeps one-metre tile seams from
    // catching tracks. Every visible tile has its top at the same y = 0.
    author.collider(
        arena,
        "Arena Floor",
        vec3(0.0, -0.5, 0.0),
        vec3(60.0, 1.0, 64.0),
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
    // Northeast x=8..24, z=14..28 (world z=36..50) stays clear for the
    // separately authored ramp/platform, south of the perimeter wall.
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
