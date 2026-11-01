// The PBR material's uniform, `@group(3) @binding(2)` of `shaders/mesh/pbr.wgsl`. The masked
// shadow pass, `shaders/mesh/shadow_masked.wgsl`, reads the same material bind group.

// glTF metallic-roughness inputs. Fields are only ever appended, so the offsets a saved
// material stores stay valid. A material saved before a field existed gets the field's
// default when it loads (see `Material`), and every default renders as the material did
// before the field existed.
struct MaterialProperties {
    // Metallic and roughness factors, multiplied by the metallic-roughness texture's B and G.
    metallic: f32,
    roughness: f32,
    // Multiplies the ambient light, on top of the occlusion texture.
    ambient_occlusion: f32,
    // Linear HDR radiance added on top of the lit color:
    // emission_color * emission_strength * emissive_texture.
    emission_strength: f32,
    emission_color: vec4f,
    // Multiplied into the base color texture. Defaults to white.
    base_color_factor: vec4f,
    // Scales the normal texture's X and Y, as glTF's normalTexture.scale. Defaults to 1.
    normal_scale: f32,
    // How much of the occlusion texture's R applies, as glTF's occlusionTexture.strength.
    // Defaults to 1.
    occlusion_strength: f32,
    // Fragments whose base color alpha is below this are discarded, as glTF's alphaCutoff in
    // MASK mode. Defaults to 0, which never discards.
    alpha_cutoff: f32,
};
