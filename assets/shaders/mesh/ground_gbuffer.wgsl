//#include "shaders/mesh/ground_surface.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// The blended ground's geometry pass: writes the ground into the G-buffer, as
// `shaders/mesh/gbuffer.wgsl` writes a PBR surface, so the deferred lighting pass lights and
// shadows it and SSAO darkens it. The targets and their channels are laid out in
// `docs/design/rendering/gbuffer.md`.
//
// Group 2 is the material's own bind group, built for group 3 of `shaders/mesh/ground.wgsl`:
// every binding is declared as there, so the two layouts match.

@group(2) @binding(0)
var mask_a_texture: texture_2d<f32>;

@group(2) @binding(1)
var mask_a_sampler: sampler;

@group(2) @binding(2)
var<uniform> material: GroundProperties;

@group(2) @binding(3)
var mask_b_texture: texture_2d<f32>;

@group(2) @binding(4)
var mask_b_sampler: sampler;

@group(2) @binding(5)
var detail_a_texture: texture_2d<f32>;

@group(2) @binding(6)
var detail_a_sampler: sampler;

@group(2) @binding(7)
var detail_b_texture: texture_2d<f32>;

@group(2) @binding(8)
var detail_b_sampler: sampler;

@group(2) @binding(9)
var marks_texture: texture_2d<f32>;

@group(2) @binding(10)
var marks_sampler: sampler;

struct GBufferOut {
    @location(0) albedo: vec4f,
    @location(1) normals: vec4f,
    @location(2) material: vec4f,
    @location(3) emissive: vec4f,
};

@fragment
fn fs_main(in: PbrVertexOut) -> GBufferOut {
    let c = ground_coords(material, in.world_position, in.uv);
    var t: GroundTexels;
    t.mask_a = textureSampleGrad(mask_a_texture, mask_a_sampler, c.mask_uv, c.mask_dx, c.mask_dy);
    t.mask_b = textureSampleGrad(mask_b_texture, mask_b_sampler, c.mask_uv, c.mask_dx, c.mask_dy);
    t.field = textureSampleGrad(mask_b_texture, mask_b_sampler, c.field_uv, c.mask_dx, c.mask_dy);
    t.detail_a0 = textureSampleGrad(detail_a_texture, detail_a_sampler, fract(c.detail_uv0),
        c.detail_dx0, c.detail_dy0);
    t.detail_a1 = textureSampleGrad(detail_a_texture, detail_a_sampler, fract(c.detail_uv1),
        c.detail_dx1, c.detail_dy1);
    t.detail_b0 = textureSampleGrad(detail_b_texture, detail_b_sampler, fract(c.detail_uv0),
        c.detail_dx0, c.detail_dy0);
    t.detail_b1 = textureSampleGrad(detail_b_texture, detail_b_sampler, fract(c.detail_uv1),
        c.detail_dx1, c.detail_dy1);
    t.marks = textureSampleGrad(marks_texture, marks_sampler, c.marks_uv, c.marks_dx, c.marks_dy);
    let surface = ground_surface(material, t, c);
    let world_normal = ground_normal(material, in.normal);
    let normal = gbuffer_encode_normal(normalize((camera.view * vec4f(world_normal, 0.0)).xyz));
    var out: GBufferOut;
    out.albedo = vec4f(surface.albedo, 1.0);
    out.normals = vec4f(normal, normal);
    out.material = vec4f(0.0, surface.roughness, 1.0, f32(min(material.outline_style, 255u)) / 255.0);
    out.emissive = vec4f(0.0, 0.0, 0.0, 1.0);
    return out;
}
