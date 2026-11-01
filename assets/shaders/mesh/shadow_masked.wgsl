//#include "shaders/common/inputs.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/pbr_material.wgsl"

// The shadow pass of an alpha-masked PBR material: as `shaders/mesh/shadow.wgsl`, but a fragment
// whose base color alpha is below the material's `alpha_cutoff` writes no depth, exactly where
// `shaders/mesh/pbr.wgsl` discards it, so cutout surfaces cast cutout shadows.
//
// Group 2 is the material's own bind group, built for group 3 of `pbr.wgsl`: every binding is
// declared as `pbr.wgsl` declares it, so the two layouts match, though only the base color
// texture, its sampler and the material uniform are read.

struct ShadowView {
    // World to the light's clip space.
    view_projection: mat4x4f,
};

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@group(0) @binding(0)
var<uniform> shadow_view: ShadowView;

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

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = mesh.instances[vertex.instance];
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    var out: VertexOut;
    out.position = shadow_view.view_projection * (instance.transform * local_position);
    out.uv = vertex.uv0;
    return out;
}

@fragment
fn fs_main(in: VertexOut) {
    let alpha =
        textureSample(base_color_texture, base_color_sampler, in.uv).a *
        material.base_color_factor.a;
    if alpha < material.alpha_cutoff {
        discard;
    }
}
