//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/pbr_surface.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"
//#include "shaders/mesh/grass_field.wgsl"

// Tall grass's geometry pass: writes it into the G-buffer as `shaders/mesh/gbuffer.wgsl` writes
// a PBR surface, from the same material, with two differences: the vertex stage droops and sways
// the grass in the wind and bends it away from the scene's grass benders (`grass_displacement`),
// and the fragment stage leaves out the grass fading around the benders that reveal, which
// `shaders/mesh/grass.wgsl` draws instead, alpha blended over the lit scene.
//
// Group 2 is the material's own bind group, built for group 3 of `shaders/mesh/grass.wgsl`,
// which declares it as `shaders/mesh/pbr.wgsl` does; group 3 is the grass field.

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

@group(3) @binding(0)
var<uniform> field: GrassField;

struct PbrVertexOut {
    @builtin(position) @invariant position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) tangent: vec4f,
};

@vertex
fn vs_main(vertex: VertexIn) -> PbrVertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: PbrVertexOut;
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    let local_normal = skinned_local(vertex, instance, vec4f(vertex.normal, 0.0));
    let local_tangent = skinned_local(vertex, instance, vec4f(vertex.tangent.xyz, 0.0));
    let root = instance.transform[3].xyz;
    let rest = (instance.transform * local_position).xyz;
    out.world_position = rest + grass_displacement(field, root, rest);
    out.normal = grass_normal((instance.transform * local_normal).xyz);
    out.tangent = vec4f((instance.transform * local_tangent).xyz, vertex.tangent.w);
    out.position = camera.projection * camera.view * vec4f(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

struct GBufferOut {
    // Base color, linear; the target stores it as sRGB. Alpha is 1, marking a drawn surface:
    // the target clears to 0.
    @location(0) albedo: vec4f,
    // View-space normals, octahedral: the shading normal in RG, the geometric normal in BA.
    @location(1) normals: vec4f,
    // Metallic in R, roughness in G, ambient occlusion in B, the material's outline style in A
    // as `style / 255` (styles above 255 store 255).
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
    // Grass fading around a bender that reveals is drawn by the transparent pass, blended.
    if grass_reveal(field, in.world_position) > 0.0 {
        discard;
    }
    var out: GBufferOut;
    out.albedo = vec4f(surface.albedo.rgb, 1.0);
    out.normals = vec4f(
        gbuffer_encode_normal(view_normal(surface.normal)),
        gbuffer_encode_normal(view_normal(surface.geometric_normal)),
    );
    out.material = vec4f(
        surface.metallic,
        surface.roughness,
        surface.occlusion,
        // No ink near the faded grass, so the blades it cuts off carry no ink line there.
        select(0.0, f32(min(material.outline_style, 255u)) / 255.0, grass_inked(field, in.world_position)),
    );
    out.emissive = vec4f(surface.emission, 1.0);
    return out;
}
