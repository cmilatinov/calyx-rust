//! PROTOTYPE (#610): authors the poise prototype's player prefab and test
//! scene. Run from the repository root:
//! `cargo run -p sandbox --example poise_poc`.
//!
//! - `assets/game/prefabs/poise_player.cxprefab`: the tank prefab's tank,
//!   camera rig and crosshair, re-rooted so the tank itself is unscaled (its
//!   model sits under a scaled `Model` child) and its colliders are in metres,
//!   with a kinematic rigid body that turns dynamic on a knockdown, poise in
//!   place of health, the boot and hook, and the prototype HUD.
//! - `assets/game/scenes/poise_poc.cxscene`: flat ground walled in, a few
//!   walls and obstacles, the player and an enemy dummy, both linked
//!   instances of that prefab, lit like the arena. It plays straight away,
//!   with no menu.
#![recursion_limit = "256"]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::component::{
    ColliderShape, ComponentCamera, ComponentCollider, ComponentID, ComponentMesh,
    ComponentRigidBody, ComponentTransform,
};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{TypeUuid, Uuid};
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec3, Vec3};
use rapier3d::dynamics::RigidBodyType;
use sandbox::poise::abilities::ComponentTankAbilities;
use sandbox::poise::hud::ComponentPoiseHud;
use sandbox::poise::{tank_collider, ComponentDummyDriver, ComponentPoise, TANK_COLLIDERS};
use sandbox::tank::{ComponentHealth, ComponentProjectileTarget, ComponentTankController};
use sandbox::terrain::ComponentDriveSurface;
use serde_json::{json, Value};

/// Half the walled play area's side, in metres.
const HALF: f32 = 24.0;
/// The border's solid rises far above its wall, so a thrown tank never
/// leaves the area.
const BORDER_HEIGHT: f32 = 12.0;
const WALL_THICKNESS: f32 = 1.2;

const PREFAB_TYPE: &str = "960f1d60-3ad4-4f1d-92d3-cceb0e0623d7";
const SCENE_TYPE: &str = "9946a2e7-e022-447e-8e60-528da548087f";
/// `game/meshes/arena_cube`, a unit cube.
const CUBE_MESH: &str = "4d2d87eb-60b4-4dcc-b3d4-67bc444396c9";
/// `generated/materials/sunny/ground`, Sunny Battlefield's grass.
const GROUND_MATERIAL: &str = "3a0456d8-d892-5462-b382-823fda3f541d";
/// `generated/materials/sunny/ground_paving`.
const PAVING_MATERIAL_PATH: &str = "generated/materials/sunny/ground_paving";
/// Tank body roles, from `game/materials/tanks/sunny`.
const PLAYER_2_MATERIAL_PATH: &str = "game/materials/tanks/sunny/player_2";
const TANK_BODY_PARTS: [&str; 3] = ["Body", "Turret Pivot", "Turret Head"];

fn stable(name: &str) -> Uuid {
    let hash = format!("calyx/poise_poc/{name}").bytes().fold(
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

fn asset_id(assets: &Path, name: &str) -> Uuid {
    read(&assets.join(format!("{name}.meta")))["main"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn write_meta(path: &Path, id: Uuid, name: &str, type_uuid: &str) {
    let display = name.rsplit('/').next().unwrap();
    let meta = json!({
        "main": {
            "id": id,
            "name": name,
            "display_name": display,
            "type_uuid": type_uuid,
        },
        "inner": [],
    });
    std::fs::write(path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
}

fn value<T: serde::Serialize>(component: T) -> Value {
    serde_json::to_value(component).unwrap()
}

fn transform(position: Vec3, rotation: UnitQuaternion<f32>, scale: Vec3) -> Value {
    value(ComponentTransform {
        transform: Transform::from_components(position, rotation, scale),
    })
}

fn identity(id: Uuid, name: &str) -> Value {
    json!({ "id": id, "name": name, "visible": true })
}

/// The player prefab, from the tank prefab's objects under fresh ids.
fn player_prefab(assets: &Path) -> Value {
    let tank = read(&assets.join("game/prefabs/tank.cxprefab"));
    let id_type = ComponentID::type_uuid().to_string();
    let transform_type = ComponentTransform::type_uuid().to_string();
    // Every id in the tank prefab maps to a stable new one.
    let ids: BTreeMap<String, String> = tank["data"]["components"]
        .as_object()
        .unwrap()
        .keys()
        .map(|old| (old.clone(), stable(&format!("player/{old}")).to_string()))
        .collect();
    let mut data = tank["data"].clone();
    remap(&mut data, &ids);
    let root = ids[tank["root"].as_str().unwrap()].clone();
    let find = |data: &Value, name: &str| -> String {
        data["components"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, components)| components[&id_type]["name"] == name)
            .map(|(id, _)| id.clone())
            .unwrap()
    };
    let tank_id = find(&data, "Tank");
    let components = data["components"].as_object_mut().unwrap();

    // The tank: unscaled, its own rigid body, poise instead of health, the
    // boot and hook, and the prototype HUD.
    let tank_components = components[&tank_id].as_object_mut().unwrap();
    let scale = tank_components[&transform_type]["transform"]["scale"].clone();
    tank_components[&transform_type]["transform"]["scale"] = json!([1.0, 1.0, 1.0]);
    for removed in [
        ComponentHealth::type_uuid(),
        sandbox::spawn::ComponentRespawnState::type_uuid(),
    ] {
        tank_components.remove(&removed.to_string());
    }
    for (type_uuid, component) in [
        (
            ComponentPoise::type_uuid(),
            value(ComponentPoise::default()),
        ),
        (
            ComponentTankAbilities::type_uuid(),
            value(ComponentTankAbilities::default()),
        ),
        (
            ComponentPoiseHud::type_uuid(),
            value(ComponentPoiseHud::default()),
        ),
        (
            ComponentRigidBody::type_uuid(),
            value(ComponentRigidBody {
                ty: RigidBodyType::KinematicPositionBased,
                mass: 100.0,
                gravity_scale: 1.0,
                can_sleep: false,
            }),
        ),
    ] {
        tank_components.insert(type_uuid.to_string(), component);
    }
    assert!(tank_components.contains_key(&ComponentTankController::type_uuid().to_string()));
    assert!(tank_components.contains_key(&ComponentProjectileTarget::type_uuid().to_string()));

    // The model under a scaled child, and the colliders beside it.
    let model_id = stable("player/Model").to_string();
    let mut model = serde_json::Map::new();
    model.insert(
        id_type.clone(),
        identity(model_id.parse().unwrap(), "Model"),
    );
    model.insert(
        transform_type.clone(),
        json!({
            "transform": {
                "position": [0.0, 0.0, 0.0],
                "rotation": [0.0, 0.0, 0.0, 1.0],
                "scale": scale,
            }
        }),
    );
    components.insert(model_id.clone(), Value::Object(model));
    let mut collider_ids = Vec::new();
    for (name, center, half_extents) in TANK_COLLIDERS {
        let id = stable(&format!("player/{name}"));
        let mut object = serde_json::Map::new();
        object.insert(id_type.clone(), identity(id, name));
        object.insert(
            transform_type.clone(),
            transform(center, UnitQuaternion::identity(), vec3(1.0, 1.0, 1.0)),
        );
        object.insert(
            ComponentCollider::type_uuid().to_string(),
            value(tank_collider(half_extents)),
        );
        components.insert(id.to_string(), Value::Object(object));
        collider_ids.push(json!(id));
    }
    let hierarchy = data["hierarchy"].as_object_mut().unwrap();
    let model_children = hierarchy.remove(&tank_id).unwrap();
    hierarchy.insert(model_id.clone(), model_children);
    let mut tank_children = vec![json!(model_id)];
    tank_children.extend(collider_ids);
    hierarchy.insert(tank_id.clone(), Value::Array(tank_children));
    json!({ "data": data, "root": root })
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

struct Author {
    scene: Scene,
    assets: PathBuf,
}

impl Author {
    fn object(&mut self, name: &str, parent: Option<GameObject>, position: Vec3) -> GameObject {
        let object = self.scene.create(
            Some(ComponentID {
                id: stable(name),
                name: name.into(),
                ..Default::default()
            }),
            parent,
        );
        self.scene
            .set_transform(object, &nalgebra_glm::translation(&position));
        object
    }

    /// A fixed box collider of `size` centred at `center`.
    fn solid(&mut self, parent: GameObject, name: &str, center: Vec3, size: Vec3) -> GameObject {
        let object = self.object(name, Some(parent), center);
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

    /// A unit cube drawn at `size` with `material`.
    fn slab(&mut self, parent: GameObject, name: &str, center: Vec3, size: Vec3, material: Uuid) {
        let object = self.object(name, Some(parent), center);
        self.scene.set_transform(
            object,
            &Transform::from_components(center, UnitQuaternion::identity(), size).matrix(),
        );
        self.scene.add_component(
            object,
            ComponentMesh {
                mesh: calyx_engine::assets::AssetRef::from_id(CUBE_MESH.parse().unwrap()),
                material: calyx_engine::assets::AssetRef::from_id(material),
                cast_shadows: false,
            },
        );
    }

    /// A linked instance of the prefab at `path` (under the assets), at
    /// `position` turned `yaw` degrees.
    fn instance(
        &mut self,
        path: &str,
        name: &str,
        parent: Option<GameObject>,
        position: Vec3,
        yaw: f32,
    ) -> GameObject {
        let source = read(&self.assets.join(format!("{path}.cxprefab")));
        let prefab = Prefab {
            data: serde_json::from_value(source["data"].clone()).unwrap(),
            root: source["root"].as_str().unwrap().parse().unwrap(),
        };
        let (object, dropped) = self
            .scene
            .instantiate_linked_prefab(asset_id(&self.assets, path), &prefab, parent)
            .unwrap();
        assert!(dropped.is_empty(), "{path} dropped components");
        self.scene
            .write_component::<ComponentID, _>(object, |id| id.name = name.into());
        let scale = self.scene.transform(object).scale;
        self.scene.set_transform(
            object,
            &Transform::from_components(
                position,
                UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
                scale,
            )
            .matrix(),
        );
        object
    }

    /// A kit prop with the solids the arena gives it.
    fn prop(&mut self, parent: GameObject, model: &str, name: &str, x: f32, z: f32, yaw: f32) {
        let root = self.instance(
            &format!("generated/prefabs/sunny/{model}"),
            name,
            Some(parent),
            vec3(x, 0.0, z),
            yaw,
        );
        let bounds = |y, w, h, d| (vec3(0.0, y, 0.0), vec3(w, h, d));
        let solids = match model {
            "wall_long" => vec![bounds(0.8, 4.0, 1.6, 1.2)],
            "wall_short" => vec![bounds(0.8, 2.0, 1.6, 1.2)],
            "barrier_hazard" | "barrier_plain" => vec![bounds(0.5, 2.5, 1.0, 1.1)],
            "crate_framed" => vec![bounds(0.55, 1.1, 1.1, 1.1)],
            "crate_large" => vec![bounds(0.825, 1.65, 1.65, 1.65)],
            "sandbag_stack" => vec![bounds(0.63, 1.94, 1.27, 0.5)],
            other => panic!("no solids for {other}"),
        };
        for (i, (center, size)) in solids.into_iter().enumerate() {
            self.solid(root, &format!("{name} solid {i}"), center, size);
        }
    }

    /// One side of the border: kit walls along it and one tall solid.
    fn border_side(&mut self, parent: GameObject, side: &str, center: Vec3, along_x: bool) {
        let length = HALF * 2.0;
        let count = (length / 4.0) as i32;
        for i in 0..count {
            let offset = -HALF + 2.0 + i as f32 * 4.0;
            let (x, z, yaw) = if along_x {
                (offset, center.z, 0.0)
            } else {
                (center.x, offset, 90.0)
            };
            self.instance(
                "generated/prefabs/sunny/wall_long",
                &format!("Border {side} {i}"),
                Some(parent),
                vec3(x, 0.0, z),
                yaw,
            );
        }
        let size = if along_x {
            vec3(length + WALL_THICKNESS, BORDER_HEIGHT, WALL_THICKNESS)
        } else {
            vec3(WALL_THICKNESS, BORDER_HEIGHT, length + WALL_THICKNESS)
        };
        self.solid(
            parent,
            &format!("Border {side} solid"),
            vec3(center.x, BORDER_HEIGHT * 0.5, center.z),
            size,
        );
    }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets = root.join("assets");

    // The player prefab.
    let prefab_name = "game/prefabs/poise_player";
    let prefab_path = assets.join(format!("{prefab_name}.cxprefab"));
    std::fs::write(
        &prefab_path,
        serde_json::to_string_pretty(&player_prefab(&assets)).unwrap(),
    )
    .unwrap();
    write_meta(
        &prefab_path.with_extension("meta"),
        stable("prefab"),
        prefab_name,
        PREFAB_TYPE,
    );

    let context = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    sandbox::plugin_main(&mut context.registries.types.write());
    context
        .registries
        .components
        .write()
        .refresh_class_lists(&context.registries.types.read());

    // The arena's sun, sky and post-processing, without fog of war: the
    // prototype has no sides to hide from each other.
    let arena = read(&assets.join("game/scenes/arena.cxscene"));
    let id_type = ComponentID::type_uuid().to_string();
    let fog_of_war = calyx_engine::component::ComponentFogOfWar::type_uuid().to_string();
    let mut rig = SceneData::default();
    for object in arena["components"].as_object().unwrap().values() {
        let object_name = object[&id_type]["name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if !["Sky", "Sun", "Post Process"].contains(&object_name.as_str()) {
            continue;
        }
        let new_id = stable(&object_name);
        let mut components: BTreeMap<Uuid, Value> = serde_json::from_value(object.clone()).unwrap();
        components.insert(id_type.parse().unwrap(), identity(new_id, &object_name));
        components.remove(&fog_of_war.parse().unwrap());
        rig.components.insert(new_id, components);
        rig.roots.push(new_id);
    }
    assert_eq!(rig.roots.len(), 3, "the arena's rig");
    let (scene, dropped) = Scene::load(&context.registries.lock_read(), rig);
    assert!(dropped.is_empty(), "the rig dropped components");
    let mut author = Author {
        scene,
        assets: assets.clone(),
    };

    // Flat ground, a paved square in the middle, and the floor tanks drive on.
    let level = author.object("Test Ground", None, Vec3::zeros());
    let paving = asset_id(&assets, PAVING_MATERIAL_PATH);
    author.slab(
        level,
        "Ground",
        vec3(0.0, -0.5, 0.0),
        vec3(120.0, 1.0, 120.0),
        GROUND_MATERIAL.parse().unwrap(),
    );
    author.slab(
        level,
        "Ground Square",
        vec3(0.0, -0.49, 0.0),
        vec3(16.0, 1.0, 16.0),
        paving,
    );
    let floor = author.solid(
        level,
        "Floor",
        vec3(0.0, -0.5, 0.0),
        vec3(HALF * 2.0 + 4.0, 1.0, HALF * 2.0 + 4.0),
    );
    author.scene.add_component(floor, ComponentDriveSurface {});

    // The border, walled in on every side.
    let border = author.object("Border", Some(level), Vec3::zeros());
    author.border_side(border, "North", vec3(0.0, 0.0, HALF), true);
    author.border_side(border, "South", vec3(0.0, 0.0, -HALF), true);
    author.border_side(border, "East", vec3(HALF, 0.0, 0.0), false);
    author.border_side(border, "West", vec3(-HALF, 0.0, 0.0), false);

    // A few walls and obstacles to throw tanks into.
    let cover = author.object("Obstacles", Some(level), Vec3::zeros());
    for (model, name, x, z, yaw) in [
        ("wall_long", "Wall West A", -14.0, 6.0, 0.0),
        ("wall_long", "Wall West B", -10.0, 6.0, 0.0),
        ("wall_long", "Wall East A", 12.0, -6.0, 90.0),
        ("wall_long", "Wall East B", 12.0, -10.0, 90.0),
        ("wall_long", "Wall North", 4.0, 15.0, 0.0),
        ("crate_large", "Crates A", -15.0, -12.0, 10.0),
        ("crate_framed", "Crates B", -13.2, -12.6, 35.0),
        ("crate_large", "Crates C", 15.0, 14.0, -15.0),
        ("sandbag_stack", "Sandbags A", -4.0, -15.0, 0.0),
        ("sandbag_stack", "Sandbags B", 18.0, 3.0, 90.0),
        ("barrier_hazard", "Barrier A", -17.0, 16.0, 45.0),
        ("barrier_hazard", "Barrier B", 6.0, -17.0, 0.0),
    ] {
        author.prop(cover, model, name, x, z, yaw);
    }

    // The player, facing north, and the enemy dummy ahead of it.
    author.instance(prefab_name, "Player", None, vec3(0.0, 0.0, -10.0), 0.0);
    let enemy = author.instance(prefab_name, "Enemy", None, vec3(0.0, 0.0, 4.0), 180.0);
    let player_2 = asset_id(&assets, PLAYER_2_MATERIAL_PATH);
    let descendants: Vec<_> = author.scene.descendants(enemy).collect();
    for object in descendants {
        let name = author.scene.name(object).to_string();
        if name == "Tank" {
            author
                .scene
                .add_component(object, ComponentDummyDriver::default());
        }
        if name == "Camera" {
            author
                .scene
                .set_component_enabled::<ComponentCamera>(object, false);
        }
        if name == "Crosshair" {
            author
                .scene
                .write_component::<ComponentID, _>(object, |id| id.visible = false);
        }
        if TANK_BODY_PARTS.contains(&name.as_str()) {
            author
                .scene
                .write_component::<ComponentMesh, _>(object, |mesh| {
                    mesh.material = calyx_engine::assets::AssetRef::from_id(player_2);
                });
        }
    }

    let scene_name = "game/scenes/poise_poc";
    let scene_path = assets.join(format!("{scene_name}.cxscene"));
    let data = SceneData::from(&author.scene);
    std::fs::write(&scene_path, serde_json::to_string_pretty(&data).unwrap()).unwrap();
    write_meta(
        &scene_path.with_extension("meta"),
        stable("scene"),
        scene_name,
        SCENE_TYPE,
    );
    println!(
        "Authored {} and {} ({} objects)",
        prefab_path.display(),
        scene_path.display(),
        data.components.len()
    );
}
