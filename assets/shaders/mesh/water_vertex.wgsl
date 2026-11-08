//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/water_material.wgsl"

// The `vs_main` vertex entry point of the water shaders: the forward pass,
// `shaders/mesh/water.wgsl`, and the geometry pass, `shaders/mesh/water_gbuffer.wgsl`. Each
// declares the material uniform `water` in its own group.
//
// The mesh is `math::WaterGrid`'s: three vertices of its own per triangle, on local Y = 0. UV0
// holds the facet's distance to the shore and a pseudo-random value, UV1 and UV2 the local XZ
// rest positions of the triangle's next and previous corners. Every vertex moves its own corner
// and both others by the same wave, so the three agree on the moved facet's normal and the
// surface stays flat-shaded and watertight as it moves.

struct WaterVertexOut {
    @builtin(position) @invariant position: vec4f,
    @location(0) world_position: vec3f,
    // World normal of the moved facet.
    @location(1) @interpolate(flat) normal: vec3f,
    // World normal of the surface at rest.
    @location(2) @interpolate(flat) rest_normal: vec3f,
    // World position of the moved facet's centroid.
    @location(3) @interpolate(flat) centroid: vec3f,
    // The facet's distance to the shore, its pseudo-random value, and the wave's height at its
    // centroid from -1 (trough) to 1 (crest).
    @location(4) @interpolate(flat) facet: vec3f,
};

const WATER_TAU = 6.283185307179586;

// The wave's local height at local `xz`: three crossing trains, the main one `wave_length` long
// and two shorter ones, each going round a whole number of times as `wave_phase` runs from 0
// to 1, so the motion loops.
fn water_wave_height(xz: vec2f) -> f32 {
    let k = WATER_TAU / max(water.wave_length, 0.01);
    let phase = WATER_TAU * water.wave_phase;
    let a = sin(k * dot(xz, vec2f(0.8, 0.6)) - phase);
    let b = sin(k * 1.43 * dot(xz, vec2f(-0.53, 0.848)) - 2.0 * phase + 1.9);
    let c = sin(k * 0.67 * dot(xz, vec2f(0.96, -0.28)) - phase + 4.2);
    return water.wave_amplitude * (0.5 * a + 0.3 * b + 0.2 * c);
}

fn water_moved(xz: vec2f) -> vec3f {
    return vec3f(xz.x, water_wave_height(xz), xz.y);
}

@vertex
fn vs_main(vertex: VertexIn) -> WaterVertexOut {
    let instance = mesh.instances[vertex.instance];
    let own = water_moved(vertex.position.xz);
    let next = water_moved(vertex.uv1);
    let previous = water_moved(vertex.uv2);
    // The triangle runs counter-clockwise seen from above, so this faces up.
    let local_normal = cross(previous - own, next - own);
    let centroid = (own + next + previous) / 3.0;
    var out: WaterVertexOut;
    out.world_position = (instance.transform * vec4f(own, 1.0)).xyz;
    out.normal = normalize((instance.transform * vec4f(local_normal, 0.0)).xyz);
    out.rest_normal = normalize((instance.transform * vec4f(0.0, 1.0, 0.0, 0.0)).xyz);
    out.centroid = (instance.transform * vec4f(centroid, 1.0)).xyz;
    out.facet = vec3f(
        vertex.uv0.x,
        vertex.uv0.y,
        clamp(centroid.y / max(water.wave_amplitude, 1e-4), -1.0, 1.0),
    );
    out.position = camera.projection * camera.view * vec4f(out.world_position, 1.0);
    return out;
}
