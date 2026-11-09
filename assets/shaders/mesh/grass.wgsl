//#include "shaders/common/inputs.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/pbr_surface.wgsl"
//#include "shaders/common/pbr_lighting.wgsl"
//#include "shaders/mesh/grass_field.wgsl"

// Tall grass's material shader, and its transparent pass. Its materials are PBR materials,
// declared as `shaders/mesh/pbr.wgsl` declares them, so their bind group fits both passes:
// - the geometry pass draws them with `shaders/mesh/grass_gbuffer.wgsl`, opaque, everywhere the
//   grass does not fade;
// - this shader draws them after the lighting pass, alpha blended, only where the grass fades
//   around a bender that reveals (`grass_reveal`), easing from opaque at the edge of its reveal
//   radius to `GRASS_REVEAL_ALPHA`. It is lit as `pbr.wgsl` lights a surface, through
//   `pbr_lighting`, so the faded grass matches the opaque grass round it; it draws no ink.
// Both share the vertex stage's droop, sway and bend (`grass_displacement`), read from the grass
// field at group 0 binding 8.

@group(0) @binding(8)
var<uniform> field: GrassField;

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

struct GrassVertexOut {
    @builtin(position) @invariant position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) tangent: vec4f,
};

@vertex
fn vs_main(vertex: VertexIn) -> GrassVertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: GrassVertexOut;
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    let local_normal = skinned_local(vertex, instance, vec4f(vertex.normal, 0.0));
    let local_tangent = skinned_local(vertex, instance, vec4f(vertex.tangent.xyz, 0.0));
    let root = instance.transform[3].xyz;
    let rest = (instance.transform * local_position).xyz;
    out.world_position = rest + grass_displacement(field, root, rest);
    out.normal = (instance.transform * local_normal).xyz;
    out.tangent = vec4f((instance.transform * local_tangent).xyz, vertex.tangent.w);
    out.position = camera.projection * camera.view * vec4f(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

@fragment
fn fs_main(in: GrassVertexOut) -> @location(0) vec4f {
    // Every texture is sampled up front: sampling needs uniform control flow.
    var texels: PbrTexels;
    texels.base_color = textureSample(base_color_texture, base_color_sampler, in.uv);
    texels.metallic_roughness =
        textureSample(metallic_roughness_texture, metallic_roughness_sampler, in.uv);
    texels.normal = textureSample(normal_texture, normal_sampler, in.uv).xyz;
    texels.emissive = textureSample(emissive_texture, emissive_sampler, in.uv).rgb;
    texels.occlusion = textureSample(occlusion_texture, occlusion_sampler, in.uv).r;
    let surface = pbr_surface(material, texels, in.normal, in.tangent);
    let reveal = grass_reveal(field, in.world_position);
    // The geometry pass drew the grass that does not fade.
    if reveal <= 0.0 || pbr_discards(material, surface) {
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
    return vec4f(pbr_lighting(point), grass_alpha(reveal));
}
