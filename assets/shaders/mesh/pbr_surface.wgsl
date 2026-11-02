//#include "shaders/mesh/pbr_material.wgsl"

// The PBR material's inputs at one fragment, read from its texels as both the forward pass,
// `shaders/mesh/pbr.wgsl`, and the G-buffer's geometry pass, `shaders/mesh/gbuffer.wgsl`, read
// them. Each shader samples the material textures of its own bind group, then calls
// `pbr_surface`, so the two passes see the same surface.

// The material textures' texels at the fragment. The color textures, base color and emissive,
// sample as sRGB, the others as linear (see `Material`).
struct PbrTexels {
    base_color: vec4f,
    // glTF packing: roughness in G, metallic in B.
    metallic_roughness: vec4f,
    // Tangent-space normal, +Y up the image.
    normal: vec3f,
    emissive: vec3f,
    // Ambient occlusion in R.
    occlusion: f32,
};

struct PbrSurface {
    // Base color texture times `base_color_factor`. A fragment whose alpha is below the
    // material's `alpha_cutoff` is discarded (see `pbr_discards`).
    albedo: vec4f,
    // World-space shading normal: the vertex normal bent by the normal texture.
    normal: vec3f,
    // World-space geometric normal, the interpolated vertex normal. Shadow lookups offset along
    // it.
    geometric_normal: vec3f,
    metallic: f32,
    roughness: f32,
    // Multiplies the ambient and image-based light.
    occlusion: f32,
    // Linear HDR radiance.
    emission: vec3f,
};

// The shading normal: the vertex normal, bent by the normal texture in the tangent frame.
// `tangent_in` is the interpolated vertex tangent and `sampled` the normal texture's texel.
fn shading_normal(
    normal: vec3f,
    tangent_in: vec4f,
    sampled: vec3f,
    normal_scale: f32,
) -> vec3f {
    let n = normalize(normal);
    let tangent = tangent_in.xyz - n * dot(n, tangent_in.xyz);
    let tangent_length = length(tangent);
    if tangent_length < 1e-6 {
        return n;
    }
    let t = tangent / tangent_length;
    let b = cross(n, t) * select(1.0, -1.0, tangent_in.w < 0.0);
    let tangent_normal = (sampled * 2.0 - 1.0) * vec3f(normal_scale, normal_scale, 1.0);
    let bent = mat3x3f(t, b, n) * tangent_normal;
    if dot(bent, bent) < 1e-12 {
        return n;
    }
    return normalize(bent);
}

// The surface `material` describes at a fragment with `texels`, the interpolated vertex
// `normal` and `tangent`.
fn pbr_surface(
    material: MaterialProperties,
    texels: PbrTexels,
    normal: vec3f,
    tangent: vec4f,
) -> PbrSurface {
    var surface: PbrSurface;
    surface.albedo = texels.base_color * material.base_color_factor;
    surface.normal = shading_normal(normal, tangent, texels.normal, material.normal_scale);
    surface.geometric_normal = normalize(normal);
    surface.metallic = material.metallic * texels.metallic_roughness.b;
    surface.roughness = material.roughness * texels.metallic_roughness.g;
    surface.occlusion = material.ambient_occlusion *
        (1.0 + material.occlusion_strength * (texels.occlusion - 1.0));
    surface.emission =
        material.emission_color.rgb * material.emission_strength * texels.emissive;
    return surface;
}

// Whether `material` discards a fragment of `surface`: its base color alpha is below the
// `alpha_cutoff`, as glTF's MASK mode.
fn pbr_discards(material: MaterialProperties, surface: PbrSurface) -> bool {
    return surface.albedo.a < material.alpha_cutoff;
}
