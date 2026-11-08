//! Build the water look-dev scene: a sunken creek with banks under each
//! combat palette, with the palette's water colours, its look-dev rig and the
//! gameplay camera's Far framing and its closest zoom.
//! Run from the repository root: cargo run -p sandbox --example water_test.
#![recursion_limit = "256"]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{ComponentCamera, ComponentID, ComponentMesh, ComponentWater};
use calyx_engine::math::Transform;
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{TypeUuid, Uuid};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec3};
use sandbox::tank_camera::ComponentTankCamera;
use serde_json::{json, Value};

/// The engine's unit cube, centred on its origin.
const CUBE: &str = "4d2d87eb-60b4-4dcc-b3d4-67bc444396c9";
/// How far apart the palettes' groups stand along X, so no camera sees
/// another palette's ground.
const GROUP_SPACING: f32 = 150.0;
/// The creek's width between its banks, and its length.
const CREEK_WIDTH: f32 = 8.0;
const CREEK_LENGTH: f32 = 140.0;
/// The creek's yaw in degrees, so its banks cross the view on a diagonal.
const CREEK_YAW: f32 = 25.0;
/// Top of the banks, the ground every prop stands on.
const BANK_TOP: f32 = 0.0;
/// The water's rest surface, below the banks: the creek is sunken.
const WATER_LEVEL: f32 = -0.6;
/// Thickness of the ground layer on top of each bank's dirt body.
const GROUND_LAYER: f32 = 0.12;
/// Top of the creek bed, under the water.
const BED_TOP: f32 = -1.2;

/// One combat palette: its folder of generated kit and materials, the
/// group's name in the look-dev scene whose rig it reuses, and its water.
struct Palette {
    name: &'static str,
    folder: &'static str,
    water: WaterColours,
}

/// A palette's water, as `docs/design/art/tank-game-direction.md` lists it.
struct WaterColours {
    deep: Color32,
    shallow: Color32,
    foam: Color32,
    glint_strength: f32,
}

const PALETTES: [Palette; 3] = [
    Palette {
        name: "Sunny Battlefield",
        folder: "sunny",
        water: WaterColours {
            deep: Color32::from_rgb(0x2F, 0x7C, 0xA0),
            shallow: Color32::from_rgb(0x5D, 0xB4, 0xBF),
            foam: Color32::from_rgb(0xE6, 0xF4, 0xEE),
            glint_strength: 2.5,
        },
    },
    Palette {
        name: "Sunbaked",
        folder: "sunbaked",
        water: WaterColours {
            deep: Color32::from_rgb(0x2E, 0x74, 0x7A),
            shallow: Color32::from_rgb(0x6A, 0xA8, 0x9C),
            foam: Color32::from_rgb(0xEC, 0xE6, 0xD6),
            glint_strength: 2.5,
        },
    },
    Palette {
        name: "Moonlit",
        folder: "moonlit",
        water: WaterColours {
            deep: Color32::from_rgb(0x1B, 0x35, 0x4E),
            shallow: Color32::from_rgb(0x2C, 0x5A, 0x73),
            foam: Color32::from_rgb(0x9C, 0xBD, 0xCC),
            glint_strength: 5.0,
        },
    },
];

fn stable(name: &str) -> Uuid {
    let hash = format!("calyx/water_test/{name}").bytes().fold(
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

fn pose(position: Vec3, yaw: f32, scale: Vec3) -> nalgebra_glm::Mat4 {
    Transform::from_components(
        position,
        UnitQuaternion::from_euler_angles(0.0, yaw.to_radians(), 0.0),
        scale,
    )
    .matrix()
}

struct Author {
    scene: Scene,
    assets: PathBuf,
    /// Prefab instance object ids, to their stable ids.
    remap: BTreeMap<String, String>,
    /// Each prefab instance object's identity as its prefab gave it, under
    /// its stable id, so the link's base hash matches after the remap.
    identity_bases: BTreeMap<Uuid, Value>,
    names: std::collections::BTreeSet<String>,
}

impl Author {
    fn claim(&mut self, name: &str) {
        assert!(self.names.insert(name.into()), "{name} is authored twice");
    }

    fn object(&mut self, name: &str, parent: Option<GameObject>, position: Vec3) -> GameObject {
        self.claim(name);
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

    /// A box of the unit cube, `size` large with its top at `top`, drawn with
    /// `palette`'s `role` material.
    fn slab(
        &mut self,
        palette: &Palette,
        name: &str,
        parent: GameObject,
        center: Vec3,
        size: Vec3,
        role: &str,
    ) {
        let object = self.object(name, Some(parent), center);
        self.scene.set_transform(object, &pose(center, 0.0, size));
        let material = self.assets.join(format!(
            "generated/materials/{}/{role}.cxmat",
            palette.folder
        ));
        self.scene.add_component(
            object,
            ComponentMesh {
                mesh: AssetRef::from_id(CUBE.parse().unwrap()),
                material: AssetRef::from_id(asset_id(&material)),
                ..Default::default()
            },
        );
    }

    /// A linked instance of `palette`'s kit `model` under `parent`.
    fn kit(
        &mut self,
        palette: &Palette,
        parent: GameObject,
        model: &str,
        name: &str,
        position: Vec3,
        yaw: f32,
        scale: f32,
    ) {
        self.claim(name);
        let path = self.assets.join(format!(
            "generated/prefabs/{}/{model}.cxprefab",
            palette.folder
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
        let members: Vec<_> = self.scene.objects().collect();
        for member in members {
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
        self.scene
            .set_transform(object, &pose(position, yaw, Vec3::repeat(scale)));
    }

    /// A camera framing `focus`, in `parent`'s frame, as the tank camera
    /// does at `zoom`.
    fn camera(&mut self, name: &str, parent: GameObject, focus: Vec3, zoom: f32) {
        let rig = ComponentTankCamera::default();
        let transform = rig.camera_transform(focus, 0.0, zoom);
        let object = self.object(name, Some(parent), Vec3::zeros());
        self.scene.set_transform(object, &transform.matrix());
        self.scene.add_component(
            object,
            ComponentCamera {
                fov: 64f32.to_radians(),
                ..Default::default()
            },
        );
    }

    /// One palette's group: its ground and the creek cut through it, the
    /// water, props on the banks and its cameras.
    fn group(&mut self, palette: &Palette, origin: Vec3) -> GameObject {
        let group = self.object(palette.name, None, origin);
        let rig = self.object(&format!("{} Rig", palette.name), Some(group), Vec3::zeros());
        self.camera(
            &format!("{} Camera Far", palette.name),
            rig,
            Vec3::zeros(),
            1.0,
        );
        self.camera(
            &format!("{} Camera Close", palette.name),
            rig,
            vec3(-1.0, 0.0, 1.0),
            0.6,
        );

        // A wide floor under everything, below the creek bed.
        self.slab(
            palette,
            &format!("{} Floor", palette.name),
            group,
            vec3(0.0, BED_TOP - 1.0, 0.0),
            vec3(200.0, 1.0, 200.0),
            "ground_dirt",
        );
        let creek = self.object(
            &format!("{} Creek", palette.name),
            Some(group),
            Vec3::zeros(),
        );
        self.scene
            .set_transform(creek, &pose(Vec3::zeros(), CREEK_YAW, Vec3::repeat(1.0)));
        // Each bank is a dirt body under a thin layer of the palette's
        // ground, so the bank wall above the water shows earth.
        let body_top = BANK_TOP - GROUND_LAYER;
        let body_depth = body_top - (BED_TOP - 0.5);
        for (side, sign) in [("West", -1.0), ("East", 1.0)] {
            let x = sign * (CREEK_WIDTH * 0.5 + 40.0);
            self.slab(
                palette,
                &format!("{} {side} Bank", palette.name),
                creek,
                vec3(x, BANK_TOP - GROUND_LAYER * 0.5, 0.0),
                vec3(80.0, GROUND_LAYER, CREEK_LENGTH),
                "ground",
            );
            self.slab(
                palette,
                &format!("{} {side} Bank Wall", palette.name),
                creek,
                vec3(x, body_top - body_depth * 0.5, 0.0),
                vec3(80.0, body_depth, CREEK_LENGTH),
                "ground_dirt",
            );
        }
        self.slab(
            palette,
            &format!("{} Creek Bed", palette.name),
            creek,
            vec3(0.0, BED_TOP - 0.5, 0.0),
            vec3(CREEK_WIDTH, 1.0, CREEK_LENGTH),
            "ground_dirt",
        );
        let water = self.object(
            &format!("{} Water", palette.name),
            Some(creek),
            vec3(0.0, WATER_LEVEL, 0.0),
        );
        self.scene.add_component(
            water,
            ComponentWater {
                size: vec2(CREEK_WIDTH, CREEK_LENGTH),
                columns: 10,
                rows: 175,
                deep_color: palette.water.deep,
                shallow_color: palette.water.shallow,
                foam_color: palette.water.foam,
                glint_strength: palette.water.glint_strength,
                ..Default::default()
            },
        );

        // Props along both banks, in the creek's frame: rocks at the water's
        // edge, a rock standing in the stream, trees and grass further back.
        let half = CREEK_WIDTH * 0.5;
        for (i, (model, x, z, yaw, scale)) in [
            ("rock_large", -half - 0.6, -6.0, 20.0, 1.0),
            ("rock_small", -half - 0.3, -3.5, 0.0, 1.0),
            ("rock_medium", half + 0.5, 3.0, -30.0, 1.0),
            ("rock_small", half + 0.2, 5.0, 60.0, 1.0),
            ("rock_medium", -half - 0.4, 9.0, 45.0, 1.0),
            ("rock_small", half + 0.3, -10.0, 10.0, 1.0),
            ("tree", -half - 5.0, -1.0, 0.0, 1.0),
            ("tree_large", half + 7.0, -5.0, 30.0, 1.0),
            ("tree_small", half + 4.0, 9.0, 15.0, 1.0),
            ("tree_small", -half - 3.5, 13.0, 70.0, 1.0),
            ("berry_bush", -half - 2.0, 4.0, 0.0, 1.0),
            ("berry_bush", half + 2.5, -1.5, 40.0, 1.0),
            ("grass_tuft", -half - 1.5, 1.0, 0.0, 1.0),
            ("grass_tuft", -half - 2.5, -8.0, 50.0, 1.0),
            ("grass_tuft", half + 1.5, 7.0, 100.0, 1.0),
            ("grass_tuft", half + 1.2, -7.5, 20.0, 1.0),
            ("crate_framed", half + 5.0, 2.0, 10.0, 1.0),
            ("barrier_plain", -half - 6.0, 7.0, 75.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            self.kit(
                palette,
                creek,
                model,
                &format!("{} Bank {i} {model}", palette.name),
                vec3(x, BANK_TOP, z),
                yaw,
                scale,
            );
        }
        self.kit(
            palette,
            creek,
            "rock_medium",
            &format!("{} Stream Rock", palette.name),
            vec3(0.8, BED_TOP, -2.0),
            15.0,
            1.4,
        );
        rig
    }
}

/// The look-dev scene's rig objects of the group named `palette`: its sun
/// or moon, sky and post-processing volume, by name, with their
/// components and the components the look-dev scene disables.
fn look_dev_rig(look_dev: &SceneData, palette: &str) -> Vec<(String, Uuid)> {
    let name = |id: &Uuid| {
        look_dev.components[id][&ComponentID::type_uuid()]["name"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let group = look_dev
        .roots
        .iter()
        .find(|root| name(root) == palette)
        .unwrap();
    let rig = look_dev.hierarchy[group]
        .iter()
        .find(|child| name(child) == "Rig")
        .unwrap();
    look_dev.hierarchy[rig]
        .iter()
        .filter(|child| {
            matches!(
                name(child).as_str(),
                "Sun" | "Moon" | "Sky" | "Post Process"
            )
        })
        .map(|child| (name(child), *child))
        .collect()
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("assets/lookdev/water_test.cxscene");
    let look_dev: SceneData = serde_json::from_str(
        &std::fs::read_to_string(root.join("assets/lookdev/tank_direction.cxscene")).unwrap(),
    )
    .unwrap();
    let assets = calyx_engine::test_support::test_asset_context_with_assets(Vec::new());
    sandbox::plugin_main(&mut assets.registries.types.write());
    assets
        .registries
        .components
        .write()
        .refresh_class_lists(&assets.registries.types.read());
    let mut author = Author {
        scene: assets.registries.lock_read().scene(),
        assets: root.join("assets"),
        remap: BTreeMap::new(),
        identity_bases: BTreeMap::new(),
        names: Default::default(),
    };
    let mut rigs = Vec::new();
    for (i, palette) in PALETTES.iter().enumerate() {
        let origin = vec3(i as f32 * GROUP_SPACING, 0.0, 0.0);
        let rig = author.group(palette, origin);
        rigs.push((palette, author.scene.uuid(rig)));
    }

    let mut value = serde_json::to_value(SceneData::from(&author.scene)).unwrap();
    remap(&mut value, &author.remap);
    let mut data: SceneData = serde_json::from_value(value).unwrap();
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

    // Each palette lights and post-processes its group with its look-dev
    // rig, copied as it is. Only Sunny Battlefield's rig and Far camera are
    // enabled; switch palettes by enabling another's.
    let camera = ComponentCamera::type_uuid();
    for (index, (palette, rig)) in rigs.into_iter().enumerate() {
        for (name, source) in look_dev_rig(&look_dev, palette.name) {
            let id = stable(&format!("{} {name}", palette.name));
            let mut components = look_dev.components[&source].clone();
            let identity = components.get_mut(&ComponentID::type_uuid()).unwrap();
            identity["id"] = json!(id);
            identity["name"] = json!(format!("{} {name}", palette.name));
            data.components.insert(id, components);
            data.hierarchy.entry(rig).or_default().push(id);
            let disabled = if index == 0 {
                Default::default()
            } else {
                look_dev
                    .disabled_components
                    .get(&source)
                    .cloned()
                    .unwrap_or_default()
            };
            if !disabled.is_empty() {
                data.disabled_components.insert(id, disabled);
            }
        }
        for view in ["Far", "Close"] {
            if index != 0 || view != "Far" {
                let id = stable(&format!("{} Camera {view}", palette.name));
                data.disabled_components
                    .entry(id)
                    .or_default()
                    .insert(camera);
            }
        }
    }

    // Saved as the editor saves it, so a save in the editor changes nothing.
    let (scene, dropped) = Scene::load(&assets.registries.lock_read(), data.clone());
    assert!(dropped.is_empty(), "the water scene lost components");
    let output = serde_json::to_string_pretty(&scene).unwrap();
    std::fs::write(&path, output).unwrap();
    println!(
        "Authored {} objects into {}",
        data.components.len(),
        path.display()
    );
}
