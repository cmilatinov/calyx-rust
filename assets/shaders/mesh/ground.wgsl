//#include "shaders/mesh/ground_surface.wgsl"
//#include "shaders/common/pbr_lighting.wgsl"

// The blended ground's forward pass: a ground material's own shader, which declares its
// bindings. The renderer draws the ground through the geometry pass,
// `shaders/mesh/ground_gbuffer.wgsl`, which binds this shader's group 3 as its group 2, so SSAO
// and the lighting pass treat it as any opaque surface; this pass lights it the same way, through
// `pbr_lighting`. The masks and detail textures are data, sampled as linear.

@group(3) @binding(0)
var mask_a_texture: texture_2d<f32>;

@group(3) @binding(1)
var mask_a_sampler: sampler;

@group(3) @binding(2)
var<uniform> material: GroundProperties;

@group(3) @binding(3)
var mask_b_texture: texture_2d<f32>;

@group(3) @binding(4)
var mask_b_sampler: sampler;

@group(3) @binding(5)
var detail_a_texture: texture_2d<f32>;

@group(3) @binding(6)
var detail_a_sampler: sampler;

@group(3) @binding(7)
var detail_b_texture: texture_2d<f32>;

@group(3) @binding(8)
var detail_b_sampler: sampler;

// Multiplies the ground's colour over `marks_rect`. Defaults to white, which leaves it as is.
@group(3) @binding(9)
var marks_texture: texture_2d<f32>;

@group(3) @binding(10)
var marks_sampler: sampler;

@fragment
fn fs_main(in: PbrVertexOut) -> @location(0) vec4f {
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
    var point: PbrShadingPoint;
    point.world_position = in.world_position;
    point.albedo = surface.albedo;
    let normal = ground_normal(material, in.normal);
    point.normal = normal;
    point.geometric_normal = normal;
    point.metallic = 0.0;
    point.roughness = surface.roughness;
    point.occlusion = 1.0;
    point.emission = vec3f(0.0);
    return vec4f(pbr_lighting(point), 1.0);
}
