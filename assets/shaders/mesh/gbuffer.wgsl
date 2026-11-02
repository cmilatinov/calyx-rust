//#include "shaders/mesh/pbr_vertex.wgsl"
//#include "shaders/mesh/pbr_surface.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// The geometry pass: writes the surface of each PBR mesh into the G-buffer, read from the
// material exactly as `shaders/mesh/pbr.wgsl` reads it, with the same vertex stage
// (`shaders/mesh/pbr_vertex.wgsl`). Fragments the forward pass discards are discarded here too.
// The targets and their channels are laid out in `docs/design/rendering/gbuffer.md`.
//
// Group 2 is the material's own bind group, built for group 3 of `pbr.wgsl`: every binding is
// declared as `pbr.wgsl` declares it, so the two layouts match.

@group(2) @binding(0)
var base_color_texture: texture_2d<f32>;

@group(2) @binding(1)
var base_color_sampler: sampler;

@group(2) @binding(2)
var<uniform> material: MaterialProperties;

@group(2) @binding(3)
var metallic_roughness_texture: texture_2d<f32>;

@group(2) @binding(4)
var metallic_roughness_sampler: sampler;

@group(2) @binding(5)
var normal_texture: texture_2d<f32>;

@group(2) @binding(6)
var normal_sampler: sampler;

@group(2) @binding(7)
var emissive_texture: texture_2d<f32>;

@group(2) @binding(8)
var emissive_sampler: sampler;

@group(2) @binding(9)
var occlusion_texture: texture_2d<f32>;

@group(2) @binding(10)
var occlusion_sampler: sampler;

struct GBufferOut {
    // Base color, linear; the target stores it as sRGB. Alpha is 1, marking a drawn surface:
    // the target clears to 0.
    @location(0) albedo: vec4f,
    // View-space normals, octahedral: the shading normal in RG, the geometric normal in BA.
    @location(1) normals: vec4f,
    // Metallic in R, roughness in G, ambient occlusion in B. Alpha is unused.
    @location(2) material: vec4f,
    // Linear HDR emission in RGB. Alpha is unused.
    @location(3) emissive: vec4f,
};

// `world`, a world-space direction, in view space.
fn view_normal(world: vec3f) -> vec3f {
    return normalize((camera.view * vec4f(world, 0.0)).xyz);
}

@fragment
fn fs_main(in: PbrVertexOut) -> GBufferOut {
    // Every texture is sampled up front: sampling needs uniform control flow.
    var texels: PbrTexels;
    texels.base_color = textureSample(base_color_texture, base_color_sampler, in.uv);
    texels.metallic_roughness =
        textureSample(metallic_roughness_texture, metallic_roughness_sampler, in.uv);
    texels.normal = textureSample(normal_texture, normal_sampler, in.uv).xyz;
    texels.emissive = textureSample(emissive_texture, emissive_sampler, in.uv).rgb;
    texels.occlusion = textureSample(occlusion_texture, occlusion_sampler, in.uv).r;
    let surface = pbr_surface(material, texels, in.normal, in.tangent);
    if pbr_discards(material, surface) {
        discard;
    }
    var out: GBufferOut;
    out.albedo = vec4f(surface.albedo.rgb, 1.0);
    out.normals = vec4f(
        gbuffer_encode_normal(view_normal(surface.normal)),
        gbuffer_encode_normal(view_normal(surface.geometric_normal)),
    );
    out.material = vec4f(surface.metallic, surface.roughness, surface.occlusion, 1.0);
    out.emissive = vec4f(surface.emission, 1.0);
    return out;
}
