//! RPG Poly Pack - Lite (Gigel3d), imported into `demos/assets/rpg_poly/`.

use std::f64::consts::PI;

use uuid::{uuid, Uuid};

use crate::json::{round_to, Json};
use crate::package::UnityPackage;
use crate::packs::Pack;
use crate::scene::{Converted, SceneWriter, UnityCamera};
use crate::yaml::Value;

pub const PACK: Pack = Pack {
    id: "rpg-poly-pack-lite",
    title: "RPG Poly Pack - Lite",
    asset_store_path: &[
        "Gigel3d",
        "3D ModelsEnvironmentsLandscapes",
        "RPG Poly Pack - Lite.unitypackage",
    ],
    root: "demos/assets/rpg_poly",
    asset_prefix: "rpg_poly",
    namespace: NAMESPACE,
    demo_scene: "Assets/RPGPP_LT/Scene/rpgpp_lt_scene_1.0.unity",
    scene_name: "rpg_poly",
    // The pack's one atlas material.
    default_material: "58d53420af384224ea4cfee8973d18cb",
    texture_fog,
    finish_scene,
};

const NAMESPACE: Uuid = uuid!("5d7c1a3e-4f0b-4b8e-9a61-413413413413");

const C_CAMERA: &str = "a85867d2-3e68-42b2-b943-ea78c7c6ddb5";
const C_SUN: &str = "72b2568a-2ea0-4f58-ae76-e3f655006f0f";
const C_SKY: &str = "eb42f81f-45ab-428c-9d07-961696edc5fa";
const C_VOLUME: &str = "cdbfef3f-81a5-4ccb-a0f6-098bc5a26979";
const C_AO: &str = "01a950bc-d431-4495-8b86-7d51c5f2b4b0";
const C_VIGNETTE: &str = "61efa204-ed0d-4d1f-86a4-b226be940b13";
const C_TONEMAP: &str = "68ac12ac-fc9a-489e-b305-363eaf593966";
const C_BLOOM: &str = "7e09858d-fd71-4729-bb90-8ed76c0bda9a";
const C_FXAA: &str = "c19ff2b7-c3ed-4f04-9dfa-e026efdedf82";
const C_GRADING: &str = "d2694fb5-4815-471e-9537-4ab502c580f0";
const C_CAMERA_PATH: &str = "b6f1c0de-2a4e-4c5b-9d37-8e1f4a6b2c90";

/// The committed sky (`sky/kloofendal_48d_partly_cloudy_puresky_2k.meta`):
/// Poly Haven's Kloofendal 48d Partly Cloudy (CC0) lights the scene's
/// ambient, standing in for the procedural sky Unity's ambient comes from.
/// The pack's own sky dome and clouds are drawn over it.
const SKYBOX_ID: &str = "b54b855c-1db7-5cdc-9293-e19639776142";

/// The sky dome's texture, and the dome's radius in the demo scene: its mesh
/// spans about 49 m, and the scene scales it by 10.
const SKY_TEXTURE: &str = "5c4fa0e44769f8f46a06d96031e56a4e";
const SKY_DOME_RADIUS: f64 = 490.0;

/// Unity's directional light of intensity 1 under its default ambient, in
/// Calyx's units; tuned by eye against the store screenshots.
const SUN_INTENSITY: f64 = 3.0;
const SKY_INTENSITY: f64 = 0.9;

/// The flythrough: from the key art's camera it climbs onto a circle above
/// the rooftops and treetops, circles the village looking at its centre, and
/// comes back down. A loop through the pack's own screenshot cameras would
/// cut through roofs and trees, since they stand among them.
const ORBIT_CENTRE: [f64; 3] = [76.0, 16.0, 40.0];
const ORBIT_RADIUS: f64 = 46.0;
const ORBIT_HEIGHT: f64 = 36.0;
const ORBIT_STOPS: usize = 8;
const FRAMING_CAMERA: &str = "Camera(KeyImage)";

/// The demo scene's linear fog where it meets the sky dome, for the dome's
/// texture. Unity draws the dome through the scene's fog, which washes its
/// saturated teal out to the pale blue of the store screenshots. Calyx has
/// no fog, and the dome sits at a near-constant distance from the village,
/// so the fog's effect on it is baked into its texture instead.
fn texture_fog(package: &UnityPackage, guid: &str) -> Result<Option<[f64; 4]>, String> {
    if guid != SKY_TEXTURE {
        return Ok(None);
    }
    let scene = package
        .guids
        .get(PACK.demo_scene)
        .ok_or("the pack has no demo scene")?;
    let documents = package.documents(scene)?;
    let Some(settings) = documents.iter().find(|d| d.class_name == "RenderSettings") else {
        return Ok(None);
    };
    let settings = &settings.body;
    if settings.str_at("m_Fog") != Some("1") {
        return Ok(None);
    }
    let color = settings.get("m_FogColor").ok_or("fog without a colour")?;
    let start = settings.f64_at("m_LinearFogStart").unwrap_or(0.0);
    let end = settings.f64_at("m_LinearFogEnd").unwrap_or(1.0);
    let fraction = ((SKY_DOME_RADIUS - start) / (end - start)).clamp(0.0, 1.0);
    Ok(Some([
        color.f64_at("r").unwrap_or(0.0),
        color.f64_at("g").unwrap_or(0.0),
        color.f64_at("b").unwrap_or(0.0),
        fraction,
    ]))
}

fn origin() -> (Json, Json, Json) {
    (
        Json::ints([0, 0, 0]),
        Json::ints([0, 0, 0, 1]),
        Json::ints([1, 1, 1]),
    )
}

fn finish_scene(writer: &mut SceneWriter, converted: &Converted) -> Result<String, String> {
    add_lighting(writer, converted)?;
    add_camera(writer, &converted.cameras)?;
    Ok(format!("; the camera path has {} stops", ORBIT_STOPS + 1))
}

fn add_lighting(writer: &mut SceneWriter, converted: &Converted) -> Result<(), String> {
    let (p, r, s) = origin();
    let environment = writer.add(
        "environment",
        "Environment",
        None,
        p,
        r,
        s,
        Vec::new(),
        true,
    );
    let sun = converted.lights.last().ok_or("the scene has no light")?;
    let light = &sun.light;
    let channel = |c: &str| {
        let value = light
            .get("m_Color")
            .and_then(|color| color.f64_at(c))
            .unwrap_or(1.0);
        Json::Int((value * 255.0).round_ties_even() as i64)
    };
    let shadows = light.get("m_Shadows").and_then(|s| s.str_at("m_Type")) != Some("0");
    writer.add(
        "sun",
        &sun.name,
        Some(&environment),
        Json::Array(sun.position.to_vec()),
        Json::Array(sun.rotation.to_vec()),
        Json::ints([1, 1, 1]),
        vec![(
            C_SUN.into(),
            Json::object([
                ("cast_shadows", Json::Bool(shadows)),
                (
                    "color",
                    Json::Array(vec![
                        channel("r"),
                        channel("g"),
                        channel("b"),
                        Json::Int(255),
                    ]),
                ),
                ("depth_bias", Json::Float(1.0)),
                (
                    "intensity",
                    Json::Float(SUN_INTENSITY * light.f64_at("m_Intensity").unwrap_or(1.0)),
                ),
                ("normal_bias", Json::Float(1.0)),
                ("shadow_distance", Json::Float(120.0)),
            ]),
        )],
        true,
    );
    let (p, r, s) = origin();
    writer.add(
        "sky",
        "Sky Light",
        Some(&environment),
        p,
        r,
        s,
        vec![(
            C_SKY.into(),
            Json::object([
                ("intensity", Json::Float(SKY_INTENSITY)),
                ("skybox", Json::Str(SKYBOX_ID.into())),
            ]),
        )],
        true,
    );
    let (p, r, s) = origin();
    writer.add(
        "post",
        "Post Process",
        Some(&environment),
        p,
        r,
        s,
        post_process(),
        true,
    );
    Ok(())
}

fn post_process() -> Vec<(String, Json)> {
    let f = Json::Float;
    vec![
        (C_VOLUME.into(), Json::object([("priority", Json::Int(0))])),
        (
            C_TONEMAP.into(),
            Json::object([
                ("exposure_ev", f(0.6)),
                ("preset", Json::Str("Aces".into())),
            ]),
        ),
        (
            C_BLOOM.into(),
            Json::object([
                ("intensity", f(0.08)),
                ("knee", f(0.5)),
                ("radius", Json::Int(6)),
                ("threshold", f(1.5)),
            ]),
        ),
        (
            C_AO.into(),
            Json::object([
                ("bias", f(0.025)),
                ("intensity", f(0.6)),
                ("radius", f(0.8)),
                ("sample_count", Json::Int(16)),
            ]),
        ),
        (
            C_GRADING.into(),
            Json::object([
                ("contrast", f(1.05)),
                ("saturation", f(1.0)),
                ("temperature", f(-0.06)),
                ("tint", f(0.0)),
            ]),
        ),
        (
            C_VIGNETTE.into(),
            Json::object([
                ("color", Json::ints([0, 0, 0, 255])),
                ("intensity", f(0.15)),
                ("smoothness", f(0.5)),
            ]),
        ),
        (
            C_FXAA.into(),
            Json::object([("edge_threshold", f(0.166)), ("subpixel", f(0.75))]),
        ),
    ]
}

/// The rotation whose +Z looks from `position` at `target`, upright, as
/// `[x, y, z, w]`.
fn facing(position: [f64; 3], target: [f64; 3]) -> [f64; 4] {
    let mut z = [
        target[0] - position[0],
        target[1] - position[1],
        target[2] - position[2],
    ];
    let length = (z[0] * z[0] + z[1] * z[1] + z[2] * z[2]).sqrt();
    z = z.map(|c| c / length);
    let mut x = [z[2], 0.0, -z[0]]; // up x z
    let length = (x[0].powi(2) + x[2].powi(2)).sqrt();
    x = x.map(|c| c / length);
    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];
    let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
    let w = (1.0 + m[0][0] + m[1][1] + m[2][2]).max(0.0).sqrt() / 2.0;
    if w > 1e-4 {
        return [
            (m[2][1] - m[1][2]) / (4.0 * w),
            (m[0][2] - m[2][0]) / (4.0 * w),
            (m[1][0] - m[0][1]) / (4.0 * w),
            w,
        ];
    }
    // A half turn: the axis is the column with the largest diagonal entry.
    let i = (0..3)
        .reduce(|best, k| if m[k][k] > m[best][best] { k } else { best })
        .unwrap_or(0);
    let mut axis = [0.0; 3];
    axis[i] = ((m[i][i] + 1.0) / 2.0).max(0.0).sqrt();
    for k in 0..3 {
        if k != i {
            axis[k] = m[i][k] / (2.0 * axis[i]);
        }
    }
    [axis[0], axis[1], axis[2], 0.0]
}

fn float_of(value: &Json) -> f64 {
    match value {
        Json::Float(f) => *f,
        Json::Int(i) => *i as f64,
        _ => 0.0,
    }
}

fn add_camera(writer: &mut SceneWriter, cameras: &[UnityCamera]) -> Result<(), String> {
    let framing = cameras
        .iter()
        .rev()
        .find(|c| c.name == FRAMING_CAMERA)
        .ok_or_else(|| format!("the scene has no {FRAMING_CAMERA}"))?;
    let (p, r, s) = origin();
    let path = writer.add(
        "camera path",
        "Camera Path",
        None,
        p,
        r,
        s,
        Vec::new(),
        true,
    );
    writer.add(
        &format!("stop:{}", framing.name),
        &framing.name,
        Some(&path),
        Json::Array(framing.position.to_vec()),
        Json::Array(framing.rotation.to_vec()),
        Json::ints([1, 1, 1]),
        Vec::new(),
        true,
    );
    let [cx, _, cz] = ORBIT_CENTRE;
    let start = (float_of(&framing.position[2]) - cz).atan2(float_of(&framing.position[0]) - cx);
    for k in 0..ORBIT_STOPS {
        let angle = start + 2.0 * PI * (k as f64 + 0.5) / ORBIT_STOPS as f64;
        let stop = [
            cx + ORBIT_RADIUS * angle.cos(),
            ORBIT_HEIGHT,
            cz + ORBIT_RADIUS * angle.sin(),
        ];
        writer.add(
            &format!("stop:orbit {k}"),
            &format!("Orbit {}", k + 1),
            Some(&path),
            Json::floats(stop.map(|c| round_to(c, 3))),
            Json::floats(facing(stop, ORBIT_CENTRE).map(|c| round_to(c, 6))),
            Json::ints([1, 1, 1]),
            Vec::new(),
            true,
        );
    }
    let camera: &Value = &framing.camera;
    let fov_y = camera.f64_at("field of view").unwrap_or(60.0).to_radians();
    // Calyx's camera `fov` sets the horizontal angle; the store shots are 16:9.
    let fov_x = 2.0 * ((fov_y / 2.0).tan() * 16.0 / 9.0).atan();
    writer.add(
        "camera",
        "Camera",
        None,
        Json::Array(framing.position.to_vec()),
        Json::Array(framing.rotation.to_vec()),
        Json::ints([1, 1, 1]),
        vec![
            (
                C_CAMERA.into(),
                Json::object([
                    ("clear_color", Json::ints([135, 190, 230, 255])),
                    ("far_plane", Json::Float(1500.0)),
                    ("fov", Json::Float(round_to(fov_x, 5))),
                    (
                        "near_plane",
                        Json::Float(camera.f64_at("near clip plane").unwrap_or(0.3)),
                    ),
                ]),
            ),
            (
                C_CAMERA_PATH.into(),
                Json::object([
                    ("path", Json::Str(path.clone())),
                    ("duration", Json::Float(90.0)),
                    ("looping", Json::Bool(true)),
                ]),
            ),
        ],
        true,
    );
    Ok(())
}
