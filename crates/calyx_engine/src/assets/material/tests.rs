use super::{Material, MaterialData, MaterialTexture, ShaderVariableValue};
use crate::assets::texture::TextureColorSpace;
use crate::assets::AssetAccess;
use crate::context::ReadOnlyAssetContext;
use crate::render::Shader;
use crate::test_support::TestAssets;
use serde_json::json;
use uuid::Uuid;

/// The PBR scene shader's asset id.
const PBR_SHADER: &str = "869cbd32-a789-f19d-3e24-5207f558d1c2";

fn pbr_from_shader(context: &ReadOnlyAssetContext) -> Material {
    let shader = context
        .registries
        .assets
        .read()
        .load::<Shader>("shaders/mesh/pbr")
        .expect("missing pbr shader");
    Material::from_shader(context, shader)
}

fn value<'a>(material: &'a Material, name: &str) -> &'a ShaderVariableValue {
    &material
        .variables
        .iter()
        .find(|variable| variable.name == name)
        .unwrap_or_else(|| panic!("the material has no {name}"))
        .value
}

fn float(material: &Material, name: &str) -> f32 {
    match value(material, name) {
        ShaderVariableValue::Float(value) => *value,
        _ => panic!("{name} is not a float"),
    }
}

fn color_texture(material: &Material, name: &str) -> [f32; 4] {
    match value(material, name) {
        ShaderVariableValue::Texture2D(MaterialTexture::Color(color)) => *color,
        _ => panic!("{name} is not a solid color texture"),
    }
}

#[test]
fn the_pbr_material_reflects_the_gltf_inputs_with_neutral_defaults() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();

    let material = pbr_from_shader(&context);

    let white = [1.0; 4];
    assert!(matches!(
        value(&material, "base_color_factor"),
        ShaderVariableValue::Color(color) if *color == white
    ));
    assert_eq!(float(&material, "normal_scale"), 1.0);
    assert_eq!(float(&material, "occlusion_strength"), 1.0);
    assert_eq!(
        float(&material, "ambient_occlusion"),
        1.0,
        "a new material takes the full ambient light"
    );
    assert_eq!(float(&material, "alpha_cutoff"), 0.0);
    assert_eq!(float(&material, "emission_strength"), 0.0);
    for texture in [
        "base_color_texture",
        "metallic_roughness_texture",
        "emissive_texture",
        "occlusion_texture",
    ] {
        assert_eq!(color_texture(&material, texture), white, "{texture}");
    }
    assert_eq!(
        color_texture(&material, "normal_texture"),
        [0.5, 0.5, 1.0, 1.0],
        "the normal texture defaults to the flat normal"
    );
    for sampler in [
        "base_color_sampler",
        "metallic_roughness_sampler",
        "normal_sampler",
        "emissive_sampler",
        "occlusion_sampler",
    ] {
        assert!(matches!(
            value(&material, sampler),
            ShaderVariableValue::Sampler
        ));
    }
}

#[test]
fn a_duplicate_starts_with_the_values_and_changes_apart_from_the_original() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut original = pbr_from_shader(&context);
    assert!(original.set_variable("roughness", ShaderVariableValue::Float(0.25)));

    let mut copy = original.duplicate(&context);
    assert_eq!(float(&copy, "roughness"), 0.25);
    assert_eq!(copy.shader.id(), original.shader.id());
    assert!(copy.set_variable("roughness", ShaderVariableValue::Float(0.75)));
    assert_eq!(float(&original, "roughness"), 0.25);
    let original_buffers = original.buffers.values().collect::<Vec<_>>();
    assert!(
        copy.buffers
            .values()
            .all(|buffer| !original_buffers.contains(&buffer)),
        "the copy writes its own buffers"
    );
}

#[test]
fn color_slots_sample_as_srgb_and_data_slots_as_linear() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();

    let material = pbr_from_shader(&context);

    let expected = [
        (0, TextureColorSpace::Srgb),
        (3, TextureColorSpace::Linear),
        (5, TextureColorSpace::Linear),
        (7, TextureColorSpace::Srgb),
        (9, TextureColorSpace::Linear),
    ];
    for (binding, color_space) in expected {
        assert_eq!(
            material.texture_color_space(3, binding),
            color_space,
            "binding {binding}"
        );
    }
}

#[test]
fn a_material_saved_before_the_gltf_inputs_loads_unchanged_and_gains_their_defaults() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let texture = "5a4bf5cf-b76a-65ce-5035-327f358f8017";
    // The showcase Fox's material as PR #424 saved it.
    let data: MaterialData = serde_json::from_value(json!({
        "shader": PBR_SHADER,
        "variables": [
            { "group": 3, "binding": 0, "offset": null, "name": "texture_diffuse",
              "span": null, "value": { "Texture2D": { "Asset": texture } } },
            { "group": 3, "binding": 1, "offset": null, "name": "sampler_diffuse",
              "span": null, "value": "Sampler" },
            { "group": 3, "binding": 2, "offset": 0, "name": "metallic", "span": 4,
              "value": { "Float": 0.0 } },
            { "group": 3, "binding": 2, "offset": 4, "name": "roughness", "span": 4,
              "value": { "Float": 0.8 } },
            { "group": 3, "binding": 2, "offset": 8, "name": "ambient_occlusion", "span": 4,
              "value": { "Float": 1.0 } }
        ]
    }))
    .expect("the saved material parses");

    let material = Material::from((&context, data));

    assert!(matches!(
        value(&material, "texture_diffuse"),
        ShaderVariableValue::Texture2D(MaterialTexture::Asset(asset))
            if asset.id() == Uuid::parse_str(texture).unwrap()
    ));
    assert_eq!(float(&material, "roughness"), 0.8);
    assert_eq!(float(&material, "ambient_occlusion"), 1.0);
    assert_eq!(
        float(&material, "emission_strength"),
        0.0,
        "a material saved without emission does not emit"
    );
    assert_eq!(float(&material, "normal_scale"), 1.0);
    assert_eq!(
        color_texture(&material, "normal_texture"),
        [0.5, 0.5, 1.0, 1.0]
    );
    assert_eq!(
        material.outline_style(),
        0,
        "a material saved before outline styles draws no outline"
    );
    let reflected = pbr_from_shader(&context);
    assert_eq!(
        material.variables.len(),
        reflected.variables.len(),
        "every input the shader declares is present once"
    );
    assert!(
        material.variables.is_sorted(),
        "variables are kept in binding order"
    );
    assert_eq!(
        material.texture_color_space(3, 0),
        TextureColorSpace::Srgb,
        "the saved base color slot samples as sRGB"
    );
    // Every binding the shader declares has a resource, so the bind groups build.
    let bindings = material
        .bind_group_entries
        .get(&3)
        .map(|entries| entries.keys().copied().collect::<Vec<_>>());
    assert_eq!(bindings, Some((0..=10).collect()));
}

#[test]
fn the_outline_style_defaults_to_none_and_round_trips_through_the_material_file() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut material = pbr_from_shader(&context);
    assert!(matches!(
        value(&material, "outline_style"),
        ShaderVariableValue::Uint(0)
    ));
    assert_eq!(material.outline_style(), 0, "a new material has no outline");

    assert!(material.set_variable("outline_style", ShaderVariableValue::Uint(3)));
    let saved = serde_json::to_value(&material).expect("the material serializes");
    let data: MaterialData = serde_json::from_value(saved).expect("the saved material parses");
    let loaded = Material::from((&context, data));

    assert_eq!(loaded.outline_style(), 3);
    assert_eq!(
        float(&loaded, "alpha_cutoff"),
        0.0,
        "the fields before it keep theirs"
    );
}

#[test]
fn the_default_material_file_loads_with_its_saved_values() {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();

    let material = context
        .registries
        .assets
        .read()
        .load::<Material>("materials/default")
        .expect("the default material loads");
    let material = material.read();

    assert_eq!(color_texture(&material, "texture_diffuse"), [1.0; 4]);
    assert_eq!(float(&material, "roughness"), 0.0);
    assert_eq!(float(&material, "ambient_occlusion"), 0.15);
    assert_eq!(float(&material, "emission_strength"), 0.0);
    assert_eq!(float(&material, "occlusion_strength"), 1.0);
}

#[test]
fn color_values_are_buffer_backed_as_vec4_f32() {
    let value = ShaderVariableValue::Color([0.25, 0.5, 0.75, 1.0]);

    assert_eq!(value.as_slice().len(), 16);
}

#[test]
fn color_named_vec4_variables_use_color_editor_values() {
    assert!(Material::is_color_variable("base_color"));
    assert!(Material::is_color_variable("albedo"));
    assert!(!Material::is_color_variable("clip_plane"));
}

#[test]
fn material_texture_color_key_clamps_and_quantizes() {
    assert_eq!(
        MaterialTexture::color_key([-1.0, 0.5, 1.0, 2.0]),
        [0, 128, 255, 255]
    );
}

#[test]
fn texture2d_accepts_asset_source_data() {
    let id = Uuid::parse_str("d2863902-8a4c-8f5b-2fef-8df1e3cf693e").unwrap();
    let value: ShaderVariableValue =
        serde_json::from_value(json!({ "Texture2D": { "Asset": id } })).unwrap();

    let ShaderVariableValue::Texture2D(MaterialTexture::Asset(asset)) = value else {
        panic!("expected texture asset source");
    };
    assert_eq!(asset.id(), id);
}

#[test]
fn texture2d_serializes_color_source_data() {
    let value = ShaderVariableValue::Texture2D(MaterialTexture::Color([0.25, 0.5, 0.75, 1.0]));

    assert_eq!(
        serde_json::to_value(value).unwrap(),
        json!({ "Texture2D": { "Color": [0.25, 0.5, 0.75, 1.0] } })
    );
}
