//! The arena's ground: one flat terrain over the whole apron, carved only
//! under the creek, whose material blends the ground layers from two masks
//! painted here. It replaces the one-metre ground tiles and their overlays,
//! so nothing lies coplanar with the ground.
//!
//! Layers, by mask channel (see `assets/shaders/mesh/ground_surface.wgsl`):
//! mask A holds dark grass, light grass, dirt and the sand lanes; mask B
//! gravel, field, mud, and in alpha whether a field's rows run along X.
//! Grass is the base wherever the masks leave room.
//!
//! Everything it writes lands in `assets/generated/ground/`: the masks, the
//! heightmap and one material per palette, `<palette>.cxmat`, over the toon
//! detail textures `asset_sources/ground/ground_detail.py` draws there.

use std::path::Path;

use calyx_engine::assets::AssetRef;
use calyx_engine::component::ComponentTerrain;
use calyx_engine::math::Transform;
use calyx_model_gen::palette::{find, linear_rgb, Map};
use image::{ImageBuffer, Luma, Rgba};
use nalgebra_glm::{vec2, vec3, Vec2};
use serde_json::{json, Value};

use super::{farmland, stable, Author};

/// The terrain's side, centred on the Arena: the apron the tiles covered.
pub const SIZE: f32 = 200.0;
/// Mask pixels per metre.
const MASK_RESOLUTION: f32 = 4.0;
/// Heightmap samples per side: a half-metre grid.
const HEIGHT_SAMPLES: u32 = 401;
/// How far the carved channel drops under the creek, below the banks' foot.
const CARVE_DEPTH: f32 = 1.3;
/// The terrain ramps down towards the creek between these distances past
/// its shore: from the ground's level at the outer one to the full depth
/// at the inner one, inside the bank wall. A ramp rather than a step, so
/// the half-metre grid follows it closely and stays under the banks' lip,
/// which covers about 0.2 to 0.7 m past the shore.
const CARVE_RAMP: (f32, f32) = (0.15, 0.55);

const TEXTURE_TYPE: &str = "8ba4ccec-85ab-45f5-b4ee-2e803ef548a2";
const MATERIAL_TYPE: &str = "f98a7f41-84d4-482d-b7af-a670b07035ae";

/// Metres one detail tile spans; `ground_detail.py` draws its marks for it.
const DETAIL_TILE: f32 = 6.0;

fn write_meta(file: &Path, assets: &Path, type_uuid: &str) -> calyx_engine::utils::Uuid {
    let name = file
        .strip_prefix(assets)
        .unwrap()
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/");
    let id = stable(&format!("asset/{name}"));
    let meta = json!({
        "main": {
            "id": id,
            "name": name,
            "display_name": file.file_stem().unwrap().to_string_lossy(),
            "type_uuid": type_uuid,
        },
        "inner": [],
    });
    std::fs::write(
        file.with_extension("meta"),
        serde_json::to_string_pretty(&meta).unwrap() + "\n",
    )
    .unwrap();
    id
}

fn hash(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((z as u32).wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(2_246_822_519));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

fn value_noise(p: Vec2, seed: u32) -> f32 {
    let (x, z) = (p.x.floor(), p.y.floor());
    let (fx, fz) = (p.x - x, p.y - z);
    let (ux, uz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (x, z) = (x as i32, z as i32);
    let a = hash(x, z, seed);
    let b = hash(x + 1, z, seed);
    let c = hash(x, z + 1, seed);
    let d = hash(x + 1, z + 1, seed);
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uz
}

/// Three octaves of value noise in 0..1, `scale` metres across.
fn noise(p: Vec2, scale: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut q = p / scale;
    for octave in 0..3 {
        sum += amplitude * value_noise(q, seed + octave);
        q = vec2(q.x * 0.8 - q.y * 0.6, q.x * 0.6 + q.y * 0.8) * 2.03;
        amplitude *= 0.5;
    }
    sum / 0.875
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The creek's shore distance ([`farmland::Creek::beyond`]), looked up
/// cheaply far from the creek: a one-metre grid of exact distances answers
/// wherever it shows the point is at least `near` beyond the shore, and the
/// exact distance is taken only closer in. A distance changes by at most
/// the distance moved, so the grid's nearest sample is within 0.71 m.
struct Shore<'a> {
    creek: &'a farmland::Creek,
    grid: Vec<f32>,
    near: f32,
}

impl<'a> Shore<'a> {
    fn new(creek: &'a farmland::Creek, near: f32) -> Self {
        let side = SIZE as i32 + 1;
        let grid = (0..side * side)
            .map(|i| {
                let p = vec2(
                    (i % side) as f32 - SIZE * 0.5,
                    (i / side) as f32 - SIZE * 0.5,
                );
                creek.beyond(p)
            })
            .collect();
        Self { creek, grid, near }
    }

    /// The exact distance when it is under `near`, else a lower bound
    /// that is at least `near`.
    fn beyond(&self, p: Vec2) -> f32 {
        let side = SIZE as i32 + 1;
        let index = |v: f32| ((v + SIZE * 0.5).round() as i32).clamp(0, side - 1);
        let coarse = self.grid[(index(p.y) * side + index(p.x)) as usize] - 0.71;
        if coarse >= self.near {
            coarse
        } else {
            self.creek.beyond(p)
        }
    }
}

/// Layer weights at one spot: mask A's four channels, then mask B's.
#[derive(Clone, Copy, Default)]
struct Weights([f32; 8]);

const DARK: usize = 0;
const LIGHT: usize = 1;
const DIRT: usize = 2;
const SAND: usize = 3;
const GRAVEL: usize = 4;
const FIELD: usize = 5;
const MUD: usize = 6;
const ROWS_ALONG_X: usize = 7;

impl Weights {
    /// Lays `layer` over what is there with weight `w`: the other layers
    /// give way in proportion.
    fn over(&mut self, layer: usize, w: f32) {
        let w = w.clamp(0.0, 1.0);
        for (i, value) in self.0.iter_mut().enumerate().take(MUD + 1) {
            if i != layer {
                *value *= 1.0 - w;
            }
        }
        self.0[layer] += (1.0 - self.0[layer]) * w;
    }
}

/// The painted layers of the arena's ground at Arena-frame point `p`, over
/// `kind`, the ground kind of the one-metre cell under it (`None` in the
/// creek's trench).
fn paint(p: Vec2, kind: Option<&str>, rows_along_x: Option<bool>, shore: &Shore) -> Weights {
    let mut w = Weights::default();
    let inside = p.x.abs() < 30.0 && p.y.abs() < 32.0;
    match kind {
        Some("dirt") => w.0[DIRT] = 1.0,
        Some("sand") => w.0[SAND] = 1.0,
        Some("field") => w.0[FIELD] = 1.0,
        // Under the spawn's paving and the road: a gravel bed.
        Some("paving" | "road" | "road_marked") => w.0[GRAVEL] = 1.0,
        None => w.0[MUD] = 1.0,
        _ => {
            // Grass, broken into darker and lighter patches of mixed
            // sizes, from a few metres to twenty.
            let n = 0.55 * noise(p, 18.0, 1) + 0.3 * noise(p, 7.0, 8) + 0.15 * noise(p, 3.0, 9);
            w.0[DARK] = smoothstep(0.58, 0.66, n);
            w.0[LIGHT] = smoothstep(0.38, 0.3, n);
            // Worn dirt here and there in the pen, where tanks turn.
            if inside {
                let worn = noise(p + vec2(40.0, 13.0), 6.0, 2);
                w.over(DIRT, smoothstep(0.74, 0.82, worn));
            }
        }
    }
    if let Some(along_x) = rows_along_x {
        w.0[ROWS_ALONG_X] = if along_x { 1.0 } else { 0.0 };
    }
    // The farmhouse's yard: dirt, gravelled by the door.
    let yard = (p - vec2(-11.0, -13.0)).abs();
    if yard.x < 4.2 && yard.y < 4.2 {
        let edge = 4.2 - yard.x.max(yard.y);
        w.over(
            DIRT,
            smoothstep(0.0, 1.2, edge + (noise(p, 2.0, 3) - 0.5) * 1.5),
        );
        w.over(
            GRAVEL,
            smoothstep(0.55, 0.7, noise(p + vec2(7.0, 3.0), 3.0, 4)),
        );
    }
    // A gravel pad under the water tower.
    let tower = (p - vec2(-26.0, -28.0)).norm() + (noise(p, 2.0, 5) - 0.5) * 1.2;
    w.over(GRAVEL, smoothstep(3.8, 3.0, tower));
    // Gravel shoulders along the spawn's paving.
    let pad = vec2((p.x).abs() - 3.0, (p.y + 23.0).abs() - 3.0);
    let pad = pad.x.max(pad.y);
    if kind != Some("paving") {
        w.over(GRAVEL, smoothstep(1.4, 0.4, pad + (noise(p, 1.5, 6) - 0.5)));
    }
    // The corners' yards, whose edges fray into the grass: worn dirt in
    // the barn yard and round the windmill, gravel under the silo and the
    // supply depot.
    for &(name, x0, z0, x1, z1) in super::layout::FOOTPRINTS.iter() {
        let layer = match name {
            "Barn" | "Windmill" => DIRT,
            "Silo" | "Depot" => GRAVEL,
            _ => continue,
        };
        let edge = (p.x - x0).min(x1 - p.x).min(p.y - z0).min(z1 - p.y);
        w.over(
            layer,
            smoothstep(0.0, 1.2, edge + (noise(p, 2.5, 12) - 0.5) * 1.6),
        );
    }
    // A gravel pad at the north spawn, rounder than the south's paving.
    let pad = (p - super::layout::NORTH_SPAWN).norm() + (noise(p, 2.0, 13) - 0.5) * 1.4;
    w.over(GRAVEL, smoothstep(3.8, 3.0, pad));
    // Darker grass under each patch of brush, to just inside its edge,
    // where the blades lean out over it, so no light ground shows between
    // them.
    for patch in super::layout::BRUSH_PATCHES.iter() {
        let outside = patch.beyond(p) + 0.6 + (noise(p, 1.5, 11) - 0.5) * 0.5;
        w.over(DARK, smoothstep(0.5, -0.2, outside));
    }
    // Mud along the creek's banks, fraying into the ground.
    let beyond = shore.beyond(p);
    let reach = 1.6 + 1.6 * noise(p, 4.0, 7);
    w.over(MUD, smoothstep(reach, reach - 0.9, beyond));
    w
}

impl Author {
    /// The ground: paints the masks and the carved heightmap, writes each
    /// palette's material, and adds the terrain drawn with this palette's.
    pub fn ground(
        &mut self,
        creek: &farmland::Creek,
        kind: &dyn Fn(i32, i32) -> Option<&'static str>,
    ) {
        // Mud reaches at most 3.2 m past the shore.
        let shore = Shore::new(creek, 3.5);
        let folder = self.assets.join("generated/ground");
        std::fs::create_dir_all(&folder).unwrap();
        let side = (SIZE * MASK_RESOLUTION) as u32;
        let mut mask_a = ImageBuffer::<Rgba<u8>, _>::new(side, side);
        let mut mask_b = ImageBuffer::<Rgba<u8>, _>::new(side, side);
        let mut weights = vec![Weights::default(); (side * side) as usize];
        for row in 0..side {
            for column in 0..side {
                // Row 0 is the +Z edge, as the terrain lays its UVs.
                let p = vec2(
                    -SIZE * 0.5 + (column as f32 + 0.5) / MASK_RESOLUTION,
                    SIZE * 0.5 - (row as f32 + 0.5) / MASK_RESOLUTION,
                );
                let (x, z) = (p.x.floor() as i32, p.y.floor() as i32);
                weights[(row * side + column) as usize] =
                    paint(p, kind(x, z), farmland::field_rows_along_x(x, z), &shore);
            }
        }
        // Soften the one-metre cells' edges: two box blurs, about 0.6 m.
        // Fields keep their crisp rectangles.
        for _ in 0..2 {
            let blurred = blur(&weights, side as usize, 2);
            for (w, b) in weights.iter_mut().zip(blurred) {
                let (field, rows) = (w.0[FIELD], w.0[ROWS_ALONG_X]);
                *w = b;
                w.0[FIELD] = field;
                w.0[ROWS_ALONG_X] = rows;
            }
        }
        for (i, w) in weights.iter().enumerate() {
            let (column, row) = (i as u32 % side, i as u32 / side);
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            mask_a.put_pixel(
                column,
                row,
                Rgba([byte(w.0[0]), byte(w.0[1]), byte(w.0[2]), byte(w.0[3])]),
            );
            mask_b.put_pixel(
                column,
                row,
                Rgba([byte(w.0[4]), byte(w.0[5]), byte(w.0[6]), byte(w.0[7])]),
            );
        }
        let mask_a_path = folder.join("mask_a.png");
        let mask_b_path = folder.join("mask_b.png");
        mask_a.save(&mask_a_path).unwrap();
        mask_b.save(&mask_b_path).unwrap();
        let mask_a_id = write_meta(&mask_a_path, &self.assets, TEXTURE_TYPE);
        let mask_b_id = write_meta(&mask_b_path, &self.assets, TEXTURE_TYPE);

        let n = HEIGHT_SAMPLES;
        let spacing = SIZE / (n - 1) as f32;
        let height = ImageBuffer::<Luma<u16>, _>::from_fn(n, n, |column, row| {
            let p = vec2(
                -SIZE * 0.5 + column as f32 * spacing,
                SIZE * 0.5 - row as f32 * spacing,
            );
            let t = (shore.beyond(p) - CARVE_RAMP.0) / (CARVE_RAMP.1 - CARVE_RAMP.0);
            Luma([(t.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16])
        });
        let height_path = folder.join("height.png");
        height.save(&height_path).unwrap();
        let height_id = write_meta(&height_path, &self.assets, TEXTURE_TYPE);

        let detail = |file: &str| {
            let path = folder.join(file);
            assert!(
                path.exists(),
                "run asset_sources/ground/ground_detail.py for {}",
                path.display()
            );
            write_meta(&path, &self.assets, TEXTURE_TYPE)
        };
        let (detail_a, detail_b) = (detail("detail_a.png"), detail("detail_b.png"));
        let mut material_id = None;
        for map in Map::ALL {
            let path = folder.join(format!("{}.cxmat", map.folder()));
            let material = material(map, [mask_a_id, mask_b_id, detail_a, detail_b]);
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&material).unwrap()
                    + "
",
            )
            .unwrap();
            let id = write_meta(&path, &self.assets, MATERIAL_TYPE);
            if map == self.palette {
                material_id = Some(id);
            }
        }
        let material_id = material_id.unwrap();

        let ground = self.object("Arena Ground", self.arena, vec3(0.0, -CARVE_DEPTH, 0.0));
        self.scene.set_transform(
            ground,
            &Transform::from_components(
                vec3(0.0, -CARVE_DEPTH, 0.0),
                Default::default(),
                vec3(1.0, 1.0, 1.0),
            )
            .matrix(),
        );
        self.scene.add_component(
            ground,
            ComponentTerrain {
                heightmap: AssetRef::from_id(height_id),
                material: AssetRef::from_id(material_id),
                size: vec2(SIZE, SIZE),
                height_scale: CARVE_DEPTH,
                cast_shadows: false,
            },
        );
    }
}

fn blur(weights: &[Weights], side: usize, radius: usize) -> Vec<Weights> {
    let pass = |src: &[Weights], horizontal: bool| {
        let mut out = vec![Weights::default(); src.len()];
        for row in 0..side {
            for column in 0..side {
                let mut sum = [0.0f32; 8];
                let mut count = 0.0;
                let at = if horizontal { column } else { row };
                for k in at.saturating_sub(radius)..=(at + radius).min(side - 1) {
                    let i = if horizontal {
                        row * side + k
                    } else {
                        k * side + column
                    };
                    for (s, v) in sum.iter_mut().zip(src[i].0) {
                        *s += v;
                    }
                    count += 1.0;
                }
                out[row * side + column] = Weights(sum.map(|s| s / count));
            }
        }
        out
    };
    let horizontal = pass(weights, true);
    pass(&horizontal, false)
}

/// The ground material on `map`: the palette's ground roles as layer
/// colours, over the masks and the toon detail textures.
fn material(
    map: Map,
    [mask_a, mask_b, detail_a, detail_b]: [calyx_engine::utils::Uuid; 4],
) -> Value {
    // How strongly the detail shades each layer: its two tones sit about
    // 6 % either side of the layer colour, its accents about 15 %. Low, so
    // open ground does not read as camouflage.
    let contrast = 0.35;
    // Detail tile metres, the second sample's scale (the two samples give
    // patches of different sizes), the broad light and shade patches' size
    // and strength.
    let detail = [DETAIL_TILE, 1.55, 22.0, 0.035];
    // Mask warp metres, the layers' blend band (narrow: toon edges), a crop
    // row's share of each metre and the mud's roughness.
    let blend = [0.9, 0.06, 0.5, 0.55];
    let color = |role: &str, scale: f64| {
        let [r, g, b] = linear_rgb(find(role).unwrap().hex(map)).unwrap();
        let round = |v: f64| (v * scale * 1e5).round() / 1e5;
        json!({"Color": [round(r), round(g), round(b), contrast]})
    };
    let uniform = |name: &str, offset: u32, span: u32, value: Value| json!({"group": 3, "binding": 2, "offset": offset, "name": name, "span": span, "value": value});
    let texture = |binding: u32, name: &str, id: calyx_engine::utils::Uuid| {
        json!({"group": 3, "binding": binding, "offset": null, "name": name, "span": null,
               "value": {"Texture2D": {"Asset": id}}})
    };
    let layers = [
        ("grass_color", "ground", 1.0),
        ("grass_dark_color", "ground_grass_dark", 1.0),
        ("grass_light_color", "ground_grass_light", 1.0),
        ("dirt_color", "ground_dirt", 1.0),
        ("sand_color", "road", 1.0),
        ("gravel_color", "ground_gravel", 1.0),
        ("field_color", "ground_field", 1.0),
        ("crop_color", "ground_crop", 1.0),
        // Wet earth: the bank's earth, darkened.
        ("mud_color", "creek_bank", 0.6),
    ];
    let mut variables = vec![
        texture(0, "mask_a_texture", mask_a),
        texture(3, "mask_b_texture", mask_b),
        texture(5, "detail_a_texture", detail_a),
        texture(7, "detail_b_texture", detail_b),
        json!({"group": 3, "binding": 9, "offset": null, "name": "marks_texture", "span": null,
               "value": {"Texture2D": {"Color": [1.0, 1.0, 1.0, 1.0]}}}),
    ];
    for (i, (name, role, scale)) in layers.iter().enumerate() {
        variables.push(uniform(name, i as u32 * 16, 16, color(role, *scale)));
    }
    variables.push(uniform("detail", 144, 16, json!({"Vec4": detail})));
    variables.push(uniform("blend", 160, 16, json!({"Vec4": blend})));
    // Lit as level ground: its only slopes are the channel under the creek.
    variables.push(uniform(
        "mask",
        176,
        16,
        json!({"Vec4": [SIZE, SIZE, 1.0, 0.0]}),
    ));
    variables.push(uniform(
        "marks_rect",
        192,
        16,
        json!({"Vec4": [0.0, 0.0, 0.0, 0.0]}),
    ));
    variables.push(uniform("roughness", 208, 4, json!({"Float": 1.0})));
    variables.push(uniform("outline_style", 212, 4, json!({"Uint": 0})));
    json!({
        "shader": "4b57dd6c-3785-5e8d-96af-64f7ae2e5561",
        "variables": variables,
    })
}
