//#include "shaders/mesh/water_surface.wgsl"
//#include "shaders/common/pbr_lighting.wgsl"

// The water's forward pass: a water material's own shader, which declares its uniform. The
// renderer draws water through the geometry pass, `shaders/mesh/water_gbuffer.wgsl`, which binds
// this shader's group 3 as its group 2; this pass lights a facet as the deferred path does,
// through `pbr_lighting`.

@group(3) @binding(0)
var<uniform> water: WaterProperties;

@fragment
fn fs_main(in: WaterVertexOut) -> @location(0) vec4f {
    var point: PbrShadingPoint;
    point.world_position = in.world_position;
    point.albedo = water_albedo(in.facet);
    point.normal = in.normal;
    point.geometric_normal = in.normal;
    point.metallic = 0.0;
    point.roughness = water.roughness;
    point.occlusion = 1.0;
    point.emission = water_glint(in);
    return vec4f(pbr_lighting(point), 1.0);
}
