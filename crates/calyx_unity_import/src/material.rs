//! Converting Unity materials (Standard and the built-in Unlit shaders) to
//! Calyx PBR materials.

use crate::json::{round_to, Json};
use crate::yaml::Value;

/// Calyx's PBR shader (`shaders/mesh/pbr`).
pub const PBR_SHADER: &str = "869cbd32-a789-f19d-3e24-5207f558d1c2";

/// Unity's built-in Unlit shaders (Unlit/Texture, Unlit/Color and the rest).
const UNLIT_SHADERS: std::ops::Range<i64> = 10750..10760;

/// The inputs of a Unity material the conversion uses.
#[derive(Clone, Debug, PartialEq)]
pub struct UnityMaterial {
    /// The GUID of the `_MainTex` texture, if any.
    pub texture: Option<String>,
    /// `_Color`, as stored (sRGB).
    pub color: [f64; 3],
    /// `_Glossiness`, Standard's smoothness.
    pub smoothness: f64,
    /// `_Metallic`.
    pub metallic: f64,
    /// `_EmissionColor` when the `_EMISSION` keyword is on.
    pub emission: Option<[f64; 3]>,
    /// The shader's built-in file id.
    pub shader: i64,
}

impl UnityMaterial {
    /// Reads the material from a `.mat` file's `Material` document body.
    pub fn from_body(body: &Value) -> Self {
        let props = body.get("m_SavedProperties");
        let named = |list: &str| -> Vec<(String, Value)> {
            props
                .and_then(|p| p.get(list))
                .map(|l| l.items().to_vec())
                .unwrap_or_default()
                .iter()
                .flat_map(|entry| {
                    entry
                        .entries()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let lookup = |list: &[(String, Value)], key: &str| {
            list.iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let textures = named("m_TexEnvs");
        let floats = named("m_Floats");
        let colors = named("m_Colors");
        let float = |key: &str, default: f64| {
            lookup(&floats, key)
                .and_then(|v| v.str().and_then(|t| t.trim().parse().ok()))
                .unwrap_or(default)
        };
        let rgb = |value: Option<Value>, default: [f64; 3]| match value {
            Some(color) => ["r", "g", "b"].map(|c| color.f64_at(c).unwrap_or(0.0)),
            None => default,
        };
        let texture = lookup(&textures, "_MainTex")
            .and_then(|main| main.get("m_Texture").cloned())
            .and_then(|tex| tex.str_at("guid").map(str::to_string));
        let keywords = body.str_at("m_ShaderKeywords").unwrap_or("");
        UnityMaterial {
            texture,
            color: rgb(lookup(&colors, "_Color"), [1.0, 1.0, 1.0]),
            smoothness: float("_Glossiness", 0.5),
            metallic: float("_Metallic", 0.0),
            emission: keywords
                .contains("_EMISSION")
                .then(|| rgb(lookup(&colors, "_EmissionColor"), [0.0, 0.0, 0.0])),
            shader: body
                .get("m_Shader")
                .and_then(|s| s.i64_at("fileID"))
                .unwrap_or(0),
        }
    }

    /// The Calyx material for this Unity material, given the asset id of its
    /// converted `_MainTex` texture.
    pub fn to_calyx(&self, texture_id: Option<&str>) -> Json {
        if UNLIT_SHADERS.contains(&self.shader) {
            // Unlit/Texture shows the texture as is; Unlit/Color its colour.
            return match texture_id {
                Some(texture) => calyx_material(MaterialInputs {
                    color: Some([0.0, 0.0, 0.0]),
                    roughness: 1.0,
                    emissive_texture: Some(texture),
                    emission: Some([1.0, 1.0, 1.0]),
                    ..Default::default()
                }),
                None => calyx_material(MaterialInputs {
                    color: Some(self.color),
                    roughness: 1.0,
                    unlit: true,
                    ..Default::default()
                }),
            };
        }
        calyx_material(MaterialInputs {
            texture: texture_id,
            color: Some(self.color),
            roughness: 1.0 - self.smoothness,
            metallic: self.metallic,
            emission: self.emission,
            ..Default::default()
        })
    }
}

/// The inputs of a Calyx PBR material.
#[derive(Clone, Copy, Debug)]
pub struct MaterialInputs<'a> {
    pub texture: Option<&'a str>,
    /// The base colour, sRGB; `None` writes integer white.
    pub color: Option<[f64; 3]>,
    pub roughness: f64,
    pub metallic: f64,
    pub emissive_texture: Option<&'a str>,
    /// The emission colour, sRGB, emitted at strength 1.
    pub emission: Option<[f64; 3]>,
    /// Keeps a Unity Unlit material's look: no lit base colour, its colour
    /// emitted as is.
    pub unlit: bool,
}

impl Default for MaterialInputs<'_> {
    fn default() -> Self {
        Self {
            texture: None,
            color: None,
            roughness: 0.85,
            metallic: 0.0,
            emissive_texture: None,
            emission: None,
            unlit: false,
        }
    }
}

/// An sRGB channel in linear space.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn texture_value(texture: Option<&str>) -> Json {
    match texture {
        Some(id) => Json::object([("Asset", Json::Str(id.to_string()))]),
        None => Json::object([("Color", Json::floats([1.0, 1.0, 1.0, 1.0]))]),
    }
}

/// A Calyx PBR material's `.cxmat` contents.
pub fn calyx_material(inputs: MaterialInputs) -> Json {
    let linear = |rgb: [f64; 3]| {
        [
            srgb_to_linear(rgb[0]),
            srgb_to_linear(rgb[1]),
            srgb_to_linear(rgb[2]),
            1.0,
        ]
    };
    let mut base = linear(inputs.color.unwrap_or([1.0, 1.0, 1.0]));
    let mut emission_color = [1.0, 1.0, 1.0, 1.0];
    let mut strength = 0.0;
    if inputs.unlit {
        emission_color = base;
        strength = 1.0;
        base = [0.0, 0.0, 0.0, 1.0];
    } else if let Some(emission) = inputs.emission {
        emission_color = linear(emission);
        strength = 1.0;
    }
    let rounded = |values: [f64; 4]| Json::floats(values.map(|v| round_to(v, 5)));
    let variable =
        |name: &str, binding: i64, offset: Option<i64>, span: Option<i64>, value: Json| {
            Json::object([
                ("group", Json::Int(3)),
                ("binding", Json::Int(binding)),
                ("offset", offset.map_or(Json::Null, Json::Int)),
                ("name", Json::Str(name.to_string())),
                ("span", span.map_or(Json::Null, Json::Int)),
                ("value", value),
            ])
        };
    let float = |value: f64| Json::object([("Float", Json::Float(value))]);
    let texture2d = |value: Json| Json::object([("Texture2D", value)]);
    Json::object([
        ("shader", Json::Str(PBR_SHADER.to_string())),
        (
            "variables",
            Json::Array(vec![
                variable(
                    "base_color_texture",
                    0,
                    None,
                    None,
                    texture2d(texture_value(inputs.texture)),
                ),
                variable("metallic", 2, Some(0), Some(4), float(inputs.metallic)),
                variable("roughness", 2, Some(4), Some(4), float(inputs.roughness)),
                variable("ambient_occlusion", 2, Some(8), Some(4), float(1.0)),
                variable("emission_strength", 2, Some(12), Some(4), float(strength)),
                variable(
                    "emission_color",
                    2,
                    Some(16),
                    Some(16),
                    Json::object([("Color", rounded(emission_color))]),
                ),
                variable(
                    "base_color_factor",
                    2,
                    Some(32),
                    Some(16),
                    Json::object([("Color", rounded(base))]),
                ),
                variable(
                    "emissive_texture",
                    7,
                    None,
                    None,
                    texture2d(texture_value(inputs.emissive_texture)),
                ),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::parse_documents;

    const STANDARD: &str = "--- !u!21 &2100000
Material:
  m_Name: wood
  m_Shader: {fileID: 46, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_TexEnvs:
    - _BumpMap:
        m_Texture: {fileID: 0}
    - _MainTex:
        m_Texture: {fileID: 2800000, guid: tex1, type: 3}
    m_Floats:
    - _Glossiness: 0.25
    - _Metallic: 0
    m_Colors:
    - _Color: {r: 1, g: 0.5, b: 0, a: 1}
    - _EmissionColor: {r: 1, g: 1, b: 1, a: 1}
";

    fn variable<'a>(material: &'a Json, name: &str) -> &'a Json {
        let Json::Object(entries) = material else {
            panic!()
        };
        let Json::Array(variables) = &entries[1].1 else {
            panic!()
        };
        variables
            .iter()
            .find(|v| matches!(v, Json::Object(e) if e[3].1 == Json::Str(name.into())))
            .and_then(|v| match v {
                Json::Object(e) => Some(&e[5].1),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn a_standard_material_reads_its_texture_colour_and_smoothness() {
        let body = &parse_documents(STANDARD)[0].body;
        let material = UnityMaterial::from_body(body);
        assert_eq!(
            material,
            UnityMaterial {
                texture: Some("tex1".into()),
                color: [1.0, 0.5, 0.0],
                smoothness: 0.25,
                metallic: 0.0,
                emission: None, // no _EMISSION keyword
                shader: 46,
            }
        );
    }

    #[test]
    fn a_standard_material_converts_smoothness_to_roughness_and_colour_to_linear() {
        let body = &parse_documents(STANDARD)[0].body;
        let calyx = UnityMaterial::from_body(body).to_calyx(Some("calyx-tex"));
        assert_eq!(
            variable(&calyx, "roughness"),
            &Json::object([("Float", Json::Float(0.75))])
        );
        assert_eq!(
            variable(&calyx, "base_color_texture"),
            &Json::object([(
                "Texture2D",
                Json::object([("Asset", Json::Str("calyx-tex".into()))])
            )])
        );
        // sRGB 0.5 is linear 0.21404.
        assert_eq!(
            variable(&calyx, "base_color_factor"),
            &Json::object([("Color", Json::floats([1.0, 0.21404, 0.0, 1.0]))])
        );
        assert_eq!(
            variable(&calyx, "emission_strength"),
            &Json::object([("Float", Json::Float(0.0))])
        );
    }

    #[test]
    fn an_unlit_colour_material_emits_its_colour_instead_of_lighting_it() {
        let text = "--- !u!21 &2100000
Material:
  m_Shader: {fileID: 10755, guid: 0000000000000000f000000000000000, type: 0}
  m_SavedProperties:
    m_Colors:
    - _Color: {r: 1, g: 1, b: 1, a: 1}
";
        let body = &parse_documents(text)[0].body;
        let calyx = UnityMaterial::from_body(body).to_calyx(None);
        assert_eq!(
            variable(&calyx, "base_color_factor"),
            &Json::object([("Color", Json::floats([0.0, 0.0, 0.0, 1.0]))])
        );
        assert_eq!(
            variable(&calyx, "emission_color"),
            &Json::object([("Color", Json::floats([1.0, 1.0, 1.0, 1.0]))])
        );
        assert_eq!(
            variable(&calyx, "emission_strength"),
            &Json::object([("Float", Json::Float(1.0))])
        );
    }
}
