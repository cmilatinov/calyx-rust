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
    assert!(material.casts_shadows());
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
    let material = UnityMaterial::from_body(body);
    assert!(
        !material.casts_shadows(),
        "Unity's Unlit shaders cast no shadow"
    );
    let calyx = material.to_calyx(None);
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
