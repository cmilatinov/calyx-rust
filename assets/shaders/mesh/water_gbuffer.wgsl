//#include "shaders/mesh/water_surface.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// The water's geometry pass: writes each water facet into the G-buffer, as
// `shaders/mesh/gbuffer.wgsl` writes a PBR surface, so the deferred lighting pass lights and
// shadows it, SSAO darkens it and the Outline effect inks it like any opaque surface. The
// targets and their channels are laid out in `docs/design/rendering/gbuffer.md`.
//
// Group 2 is the material's own bind group, built for group 3 of `shaders/mesh/water.wgsl`.

@group(2) @binding(0)
var<uniform> water: WaterProperties;

struct GBufferOut {
    @location(0) albedo: vec4f,
    @location(1) normals: vec4f,
    @location(2) material: vec4f,
    @location(3) emissive: vec4f,
};

fn water_view_normal(world: vec3f) -> vec3f {
    return normalize((camera.view * vec4f(world, 0.0)).xyz);
}

@fragment
fn fs_main(in: WaterVertexOut) -> GBufferOut {
    let normal = gbuffer_encode_normal(water_view_normal(water_normal(in)));
    var out: GBufferOut;
    out.albedo = vec4f(water_albedo(in), 1.0);
    // The shading normal is also the geometric one.
    out.normals = vec4f(normal, normal);
    out.material = vec4f(0.0, water.roughness, 1.0, f32(min(water.outline_style, 255u)) / 255.0);
    out.emissive = vec4f(water_glint(in), 1.0);
    return out;
}
