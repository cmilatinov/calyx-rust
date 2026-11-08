//! Build the water look-dev scene: a sunken, meandering creek with carved
//! banks under each combat palette, with the palette's water colours, its
//! look-dev rig, and the gameplay camera's Far framing and its closest zoom.
//! It also writes the scene's ground heightmap and its inked ground and bank
//! materials under `sandbox/assets/lookdev/water/`.
//! Run from the repository root: cargo run -p sandbox --example water_test.
#![recursion_limit = "256"]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use calyx_engine::assets::AssetRef;
use calyx_engine::component::{
    ComponentCamera, ComponentID, ComponentMesh, ComponentTerrain, ComponentWater, WaterObstacle,
    WaterPoint,
};
use calyx_engine::math::{Transform, WaterSection};
use calyx_engine::scene::{GameObject, Prefab, Scene, SceneData};
use calyx_engine::utils::{uuid_from_str, TypeUuid, Uuid};
use calyx_model_gen::palette::{self, Map};
use egui::Color32;
use nalgebra::UnitQuaternion;
use nalgebra_glm::{vec2, vec3, Vec2, Vec3};
use sandbox::tank_camera::ComponentTankCamera;
use serde_json::{json, Value};

/// The engine's unit cube, centred on its origin.
const CUBE: &str = "4d2d87eb-60b4-4dcc-b3d4-67bc444396c9";
/// How far apart the palettes' groups stand along X, so no camera sees
/// another palette's ground.
const GROUP_SPACING: f32 = 150.0;
/// Top of the banks, the ground every prop stands on.
const BANK_TOP: f32 = 0.0;
/// How far the water's rest surface sits below the bank top: the creek is
/// sunken.
const SUNKEN: f32 = 0.6;
/// The ground's terrain: its side, heightmap samples per side and how far
/// its white lies above its black.
const GROUND_SIZE: f32 = 100.0;
const GROUND_SAMPLES: u32 = 1024;
const GROUND_DROP: f32 = 2.0;
/// The ground lies this far below the banks' lip, so the lip covers the
/// edge of the trench the ground is carved with.
const GROUND_UNDER_LIP: f32 = 0.02;

/// One combat palette: its map, the group's name in the look-dev scene
/// whose rig it reuses, and its water.
struct Palette {
    name: &'static str,
    map: Map,
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
        map: Map::SunnyBattlefield,
        water: WaterColours {
            deep: Color32::from_rgb(0x2F, 0x7C, 0xA0),
            shallow: Color32::from_rgb(0x5D, 0xB4, 0xBF),
            foam: Color32::from_rgb(0xE6, 0xF4, 0xEE),
            glint_strength: 1.5,
        },
    },
    Palette {
        name: "Sunbaked",
        map: Map::Sunbaked,
        water: WaterColours {
            deep: Color32::from_rgb(0x2E, 0x74, 0x7A),
            shallow: Color32::from_rgb(0x6A, 0xA8, 0x9C),
            foam: Color32::from_rgb(0xEC, 0xE6, 0xD6),
            glint_strength: 1.5,
        },
    },
    Palette {
        name: "Moonlit",
        map: Map::Moonlit,
        water: WaterColours {
            deep: Color32::from_rgb(0x1B, 0x35, 0x4E),
            shallow: Color32::from_rgb(0x2C, 0x5A, 0x73),
            foam: Color32::from_rgb(0x9C, 0xBD, 0xCC),
            glint_strength: 5.0,
        },
    },
];

/// The creek's centreline and widths, in each group's frame: it meanders
/// across the Far view in four bends, from 6 to 10 m wide.
fn creek_points() -> Vec<WaterPoint> {
    [
        (2.0, -80.0, 7.0),
        (-5.0, -56.0, 8.0),
        (5.0, -34.0, 9.5),
        (-5.0, -12.0, 7.0),
        (5.0, 10.0, 10.0),
        (-4.5, 32.0, 6.5),
        (4.5, 54.0, 8.5),
        (-2.0, 80.0, 7.0),
    ]
    .into_iter()
    .map(|(x, z, width)| WaterPoint {
        position: vec2(x, z),
        width,
    })
    .collect()
}

/// The creek's water, banks and stream rocks for `palette`, in its group's
/// frame, before it is lowered to the water level.
fn creek(palette: &Palette, bank_material: Uuid) -> ComponentWater {
    ComponentWater {
        points: creek_points(),
        cell_size: 0.8,
        obstacles: STREAM_ROCKS
            .iter()
            .map(|&(s, across, scale)| {
                let (position, _) = along(s, across);
                WaterObstacle {
                    position,
                    radius: 0.75 * scale,
                }
            })
            .collect(),
        deep_color: palette.water.deep,
        shallow_color: palette.water.shallow,
        foam_color: palette.water.foam,
        glint_strength: palette.water.glint_strength,
        bank_material: AssetRef::from_id(bank_material),
        bank_height: SUNKEN,
        bank_depth: 0.6,
        bank_run: 0.4,
        bank_lip: 0.5,
        ..Default::default()
    }
}

/// Rocks standing in the stream: distance along the creek, share of the
/// half width across it (left positive) and scale.
const STREAM_ROCKS: [(f32, f32, f32); 2] = [(73.0, 0.25, 1.4), (100.0, -0.4, 1.0)];

/// The creek's cross-sections, as its water meshes them.
fn sections() -> Vec<WaterSection> {
    ComponentWater {
        points: creek_points(),
        cell_size: 0.8,
        ..Default::default()
    }
    .path()
    .sections()
    .expect("the creek meshes")
}

/// The point `s` along the creek and `across` its half width to the left
/// (negative to the right), and the flow's yaw there in degrees.
fn along(s: f32, across: f32) -> (Vec2, f32) {
    let sections = sections();
    let section = sections
        .iter()
        .min_by(|a, b| (a.s - s).abs().total_cmp(&(b.s - s).abs()))
        .unwrap();
    (
        section.center + section.normal * section.half_width * across,
        section.tangent.x.atan2(section.tangent.y).to_degrees(),
    )
}

/// The point `s` along the creek, `out` beyond the shore on side `side`
/// (1 left, -1 right).
fn beside(s: f32, side: f32, out: f32) -> Vec2 {
    let sections = sections();
    let section = sections
        .iter()
        .min_by(|a, b| (a.s - s).abs().total_cmp(&(b.s - s).abs()))
        .unwrap();
    section.center + section.normal * side * (section.half_width + out)
}

/// Writes `text` to `path`, and its `.meta` naming the asset by its path
/// under the asset root `assets`, as type `type_uuid`. Returns its id.
fn write_asset(assets: &Path, relative: &str, bytes: &[u8], type_uuid: &str) -> Uuid {
    let path = assets.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let id = uuid_from_str(relative);
    let name = relative.rsplit_once('.').unwrap().0;
    let display = name.rsplit_once('/').unwrap().1;
    let meta = format!(
        "{{\n  \"main\": {{\n    \"id\": \"{id}\",\n    \"name\": \"{name}\",\n    \"display_name\": \"{display}\",\n    \"type_uuid\": \"{type_uuid}\"\n  }},\n  \"inner\": []\n}}"
    );
    std::fs::write(path.with_extension("meta"), meta).unwrap();
    id
}

const MATERIAL_TYPE: &str = "f98a7f41-84d4-482d-b7af-a670b07035ae";
const TEXTURE_TYPE: &str = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2";

/// Writes `palette`'s `role` material, inked on the terrain's outline layer
/// (style 1), as `lookdev/water/<name>.cxmat`, and returns its id.
fn inked_material(assets: &Path, palette: &Palette, role: &str, name: &str) -> Uuid {
    let mut material = palette::find(role).unwrap().material(palette.map);
    for variable in material["variables"].as_array_mut().unwrap() {
        if variable["name"] == "outline_style" {
            variable["value"] = json!({ "Uint": 1 });
        }
    }
    write_asset(
        assets,
        &format!("lookdev/water/{}_{name}.cxmat", palette.map.folder()),
        serde_json::to_string_pretty(&material).unwrap().as_bytes(),
        MATERIAL_TYPE,
    )
}

/// Writes the ground's heightmap, `lookdev/water/creek_ground.png`: white
/// (the bank top) everywhere but a trench along the creek, black (the
/// bed) out to the middle of the banks' lip and rising to white before its
/// edge, so the lip covers the trench's edge. Returns its id.
fn ground_heightmap(assets: &Path, water: &ComponentWater) -> Uuid {
    let sections = water.path().sections().unwrap();
    let foot = water.bank_run * water.bank_depth / (water.bank_depth + water.bank_height);
    let lip_start = water.bank_run - foot;
    let low = lip_start + 0.15;
    let high = lip_start + water.bank_lip - 0.1;
    let samples = GROUND_SAMPLES;
    let mut image = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::new(samples, samples);
    for (column, row, pixel) in image.enumerate_pixels_mut() {
        let p = vec2(
            column as f32 / (samples - 1) as f32 * GROUND_SIZE - GROUND_SIZE * 0.5,
            GROUND_SIZE * 0.5 - row as f32 / (samples - 1) as f32 * GROUND_SIZE,
        );
        // Distance beyond the shore, along the nearest piece of the
        // centreline.
        let mut beyond = f32::MAX;
        for pair in sections.windows(2) {
            let (a, b) = (pair[0].center, pair[1].center);
            let ab = b - a;
            let f = ((p - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
            let distance = (p - (a + ab * f)).norm();
            let half = pair[0].half_width + (pair[1].half_width - pair[0].half_width) * f;
            beyond = beyond.min(distance - half);
        }
        let height = ((beyond - low) / (high - low)).clamp(0.0, 1.0);
        let height = height * height * (3.0 - 2.0 * height);
        *pixel = image::Luma([(height * 65535.0).round() as u16]);
    }
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    write_asset(assets, "lookdev/water/creek_ground.png", &png, TEXTURE_TYPE)
}

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

    /// A box of the unit cube, `size` large around `center`, drawn with
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
            palette.map.folder()
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
    #[allow(clippy::too_many_arguments)]
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
            palette.map.folder()
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

    /// One palette's group: its carved ground, the creek's water and banks,
    /// props on the banks and in the stream, and its cameras.
    fn group(&mut self, palette: &Palette, origin: Vec3, heightmap: Uuid) -> GameObject {
        let name = palette.name;
        let group = self.object(name, None, origin);
        let rig = self.object(&format!("{name} Rig"), Some(group), Vec3::zeros());
        self.camera(&format!("{name} Camera Far"), rig, Vec3::zeros(), 1.0);
        // The closest zoom, on a bend with a rock standing in the stream.
        let (rock, _) = along(STREAM_ROCKS[0].0, STREAM_ROCKS[0].1);
        self.camera(
            &format!("{name} Camera Close"),
            rig,
            vec3(rock.x, 0.0, rock.y + 1.0),
            0.6,
        );

        // A wide floor far under everything, for any gap at the ground's
        // edge.
        self.slab(
            palette,
            &format!("{name} Floor"),
            group,
            vec3(0.0, BANK_TOP - GROUND_DROP - 0.6, 0.0),
            vec3(300.0, 1.0, 300.0),
            "ground_dirt",
        );
        let assets = self.assets.clone();
        let ground_material = inked_material(&assets, palette, "ground", "ground");
        let bank_material = inked_material(&assets, palette, "ground_dirt", "bank");
        let ground = self.object(
            &format!("{name} Ground"),
            Some(group),
            vec3(0.0, BANK_TOP - GROUND_UNDER_LIP - GROUND_DROP, 0.0),
        );
        self.scene.add_component(
            ground,
            ComponentTerrain {
                heightmap: AssetRef::from_id(heightmap),
                material: AssetRef::from_id(ground_material),
                size: vec2(GROUND_SIZE, GROUND_SIZE),
                height_scale: GROUND_DROP,
                cast_shadows: false,
            },
        );
        let water = self.object(
            &format!("{name} Water"),
            Some(group),
            vec3(0.0, BANK_TOP - SUNKEN, 0.0),
        );
        self.scene
            .add_component(water, creek(palette, bank_material));

        // Props along both banks: rocks on the lip at the water's edge, trees,
        // bushes and grass further back, and cover by the bends.
        for (i, &(model, s, side, out, yaw, scale)) in [
            ("rock_large", 60.0, 1.0, 0.6, 20.0, 1.0),
            ("rock_small", 66.0, -1.0, 0.5, 0.0, 1.0),
            ("rock_medium", 84.0, -1.0, 0.7, -30.0, 1.0),
            ("rock_small", 88.0, 1.0, 0.5, 60.0, 1.0),
            ("rock_medium", 97.0, 1.0, 0.7, 45.0, 1.0),
            ("rock_small", 54.0, -1.0, 0.5, 10.0, 1.0),
            ("tree", 70.0, 1.0, 5.0, 0.0, 1.0),
            ("tree_large", 76.0, -1.0, 7.5, 30.0, 1.0),
            ("tree_small", 92.0, -1.0, 4.0, 15.0, 1.0),
            ("tree_small", 100.0, 1.0, 3.5, 70.0, 1.0),
            ("tree", 52.0, -1.0, 9.0, 40.0, 1.0),
            ("berry_bush", 80.0, 1.0, 2.0, 0.0, 1.0),
            ("berry_bush", 63.0, -1.0, 2.5, 40.0, 1.0),
            ("grass_tuft", 74.0, 1.0, 1.5, 0.0, 1.0),
            ("grass_tuft", 58.0, 1.0, 2.5, 50.0, 1.0),
            ("grass_tuft", 90.0, -1.0, 1.5, 100.0, 1.0),
            ("grass_tuft", 69.0, -1.0, 1.2, 20.0, 1.0),
            ("crate_framed", 86.0, -1.0, 6.0, 10.0, 1.0),
            ("barrier_plain", 66.0, 1.0, 9.0, 75.0, 1.0),
        ]
        .iter()
        .enumerate()
        {
            let at = beside(s, side, out);
            self.kit(
                palette,
                group,
                model,
                &format!("{name} Bank {i} {model}"),
                vec3(at.x, BANK_TOP, at.y),
                yaw,
                scale,
            );
        }
        for (i, &(s, across, scale)) in STREAM_ROCKS.iter().enumerate() {
            let (at, yaw) = along(s, across);
            self.kit(
                palette,
                group,
                "rock_medium",
                &format!("{name} Stream Rock {i}"),
                vec3(at.x, BANK_TOP - SUNKEN - 0.6 * scale, at.y),
                yaw + 15.0,
                scale,
            );
        }
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
    let heightmap = ground_heightmap(&author.assets, &creek(&PALETTES[0], Uuid::nil()));
    let mut rigs = Vec::new();
    for (i, palette) in PALETTES.iter().enumerate() {
        let origin = vec3(i as f32 * GROUP_SPACING, 0.0, 0.0);
        let rig = author.group(palette, origin, heightmap);
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
