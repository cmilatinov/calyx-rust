//#include "shaders/mesh/pbr_vertex.wgsl"
//#include "shaders/mesh/pbr_surface.wgsl"
//#include "shaders/common/pbr_lighting.wgsl"

// Tall grass's material shader: its materials are PBR materials, declared and shaded as
// `shaders/mesh/pbr.wgsl` declares and shades them, so their bind group fits both. The geometry
// pass draws a material with this shader with `shaders/mesh/grass_gbuffer.wgsl`, which sways and
// bends the grass by the scene's `ComponentGrassField` and parts it around what it reveals; this
// forward entry, a copy of `pbr.wgsl`'s, only shades it still.

// Material textures. Each sampler samples the texture bound just before it. The color
// textures, base color and emissive, sample as sRGB, and the others as linear (see `Material`).

@group(3) @binding(0)
var base_color_texture: texture_2d<f32>;

@group(3) @binding(1)
var base_color_sampler: sampler;

@group(3) @binding(2)
var<uniform> material: MaterialProperties;

// glTF packing: roughness in G, metallic in B.
@group(3) @binding(3)
var metallic_roughness_texture: texture_2d<f32>;

@group(3) @binding(4)
var metallic_roughness_sampler: sampler;

// Tangent-space normals, +Y up the image. Defaults to the flat normal.
@group(3) @binding(5)
var normal_texture: texture_2d<f32>;

@group(3) @binding(6)
var normal_sampler: sampler;

@group(3) @binding(7)
var emissive_texture: texture_2d<f32>;

@group(3) @binding(8)
var emissive_sampler: sampler;

// Ambient occlusion in R, so one texture can hold occlusion, roughness and metallic.
@group(3) @binding(9)
var occlusion_texture: texture_2d<f32>;

@group(3) @binding(10)
var occlusion_sampler: sampler;

@fragment
fn fs_main(in: PbrVertexOut) -> @location(0) vec4f {
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
    var point: PbrShadingPoint;
    point.world_position = in.world_position;
    point.albedo = surface.albedo.rgb;
    point.normal = surface.normal;
    point.geometric_normal = surface.geometric_normal;
    point.metallic = surface.metallic;
    point.roughness = surface.roughness;
    point.occlusion = surface.occlusion;
    point.emission = surface.emission;
    // Linear HDR radiance: the post-processing chain tone maps it. Every fragment kept is
    // opaque, whatever its base color alpha, as the deferred path writes it.
    return vec4f(pbr_lighting(point), 1.0);
}

