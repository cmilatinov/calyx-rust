//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/water_material.wgsl"

// The `vs_main` vertex entry point of the water shaders: the forward pass,
// `shaders/mesh/water.wgsl`, and the geometry pass, `shaders/mesh/water_gbuffer.wgsl`. Each
// declares the material uniform `water` in its own group.
//
// The mesh is `math::WaterPath`'s, built once on the CPU: three vertices of its own per triangle,
// at rest on local Y = 0. Every displacement happens here. A vertex knows its own river
// coordinates (s along the flow, t across it) and the rest positions and river coordinates of
// its triangle's other two corners, and moves all three by the same waves, so the three agree on
// the moved facet's normal: the surface stays flat-shaded and watertight as it rolls.
//
// Mesh channels (see `WaterGeometry`):
// - position: the vertex's local rest position;
// - uv0: its own river coordinates (s, t);
// - uv1, uv2: the next and previous corners' local XZ rest positions;
// - uv3: the next corner's river coordinates;
// - tangent: the previous corner's river coordinates, then the vertex's own distance to the
//   shore and the rounding of the facet's footprint, -1 for a facet with none;
// - normal: the facet's pseudo-random value and its centroid's distance to the shore and to its
//   footprint;
// - bone_weights: the vertex in the frame of the facet's footprint, then the footprint's half
//   extents. A footprint is what something standing in the water covers of it, a rounded
//   rectangle (see `math::WaterFootprint`); the water's meshes carry no bones.

struct WaterVertexOut {
    @builtin(position) @invariant position: vec4f,
    @location(0) world_position: vec3f,
    // World normal of the moved facet.
    @location(1) @interpolate(flat) normal: vec3f,
    // World normal of the surface at rest.
    @location(2) @interpolate(flat) rest_normal: vec3f,
    // World position of the moved facet's centroid.
    @location(3) @interpolate(flat) centroid: vec3f,
    // The facet's pseudo-random value, its centroid's distance to the shore, and the wave's
    // height at its centroid from -1 (trough) to 1 (crest).
    @location(4) @interpolate(flat) facet: vec3f,
    // The point's river coordinates (s, t) and its distance to the shore, interpolated across
    // the facet for the per-pixel foam.
    @location(5) river: vec3f,
    // World normal of the waves at the vertex, from their slope, interpolated: the smooth
    // shading's normal.
    @location(6) smooth_normal: vec3f,
    // The point in the frame of the facet's footprint, interpolated: linear across the facet,
    // so the distance to the footprint is exact per pixel.
    @location(7) footprint: vec2f,
    // The footprint's half extents and rounding, -1 for none, and the distance from the facet's
    // centroid to it.
    @location(8) @interpolate(flat) footprint_shape: vec4f,
};

// The signed distance from the point to the edge of its facet's footprint, negative inside, or
// a long way for a facet with none.
fn water_obstacle_distance(in: WaterVertexOut) -> f32 {
    let rounding = in.footprint_shape.z;
    if rounding < 0.0 {
        return 1e4;
    }
    let q = abs(in.footprint) - (in.footprint_shape.xy - vec2f(rounding));
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - rounding;
}

const WATER_TAU = 6.283185307179586;

// A pseudo-random value in [0, 1) for the lattice point `p`.
fn water_hash(p: vec2f) -> f32 {
    var q = fract(p * vec2f(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

// Smooth value noise in [0, 1] at `p`, one lattice cell per unit.
fn water_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = water_hash(i);
    let b = water_hash(i + vec2f(1.0, 0.0));
    let c = water_hash(i + vec2f(0.0, 1.0));
    let d = water_hash(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The waves' local height at river coordinates `river` (s, t): a main train `wave_length` long
// rolling downstream at the flow speed, a shorter train crossing it at an angle, a slow
// cross-river ripple and drifting noise, layered so the surface never repeats in an obvious way.
fn water_wave_height(river: vec2f) -> f32 {
    let s = river.x;
    let t = river.y;
    let k = WATER_TAU / max(water.wave_length, 0.1);
    let travel = water.flow_speed * water.time;
    let main = sin(k * (s - travel) + 0.3 * k * t);
    let short = sin(1.85 * k * (s - 1.3 * travel) - 0.9 * k * t + 1.7);
    let ripple = sin(0.6 * k * (s - 0.7 * travel) + 1.5 * k * t + 4.1);
    let noise = water_noise(vec2f(s, t) * (2.0 / max(water.wave_length, 0.1))
        + vec2f(-0.8 * travel, 0.2 * water.time) * (2.0 / max(water.wave_length, 0.1))) * 2.0 - 1.0;
    return water.wave_amplitude * (0.45 * main + 0.25 * short + 0.15 * ripple + 0.4 * noise);
}

// The waves' local normal at the vertex whose river coordinates are `river`, for smooth shading:
// the height's slope along s and t, by central differences, carried into local X and Z through
// the map from local XZ to river coordinates across the vertex's triangle, given by its edges to
// the next and previous corners in both spaces.
fn water_smooth_normal(river: vec2f, edges_xz: mat2x2f, edges_river: mat2x2f) -> vec3f {
    let e = 0.05;
    let slope = vec2f(
        water_wave_height(river + vec2f(e, 0.0)) - water_wave_height(river - vec2f(e, 0.0)),
        water_wave_height(river + vec2f(0.0, e)) - water_wave_height(river - vec2f(0.0, e)),
    ) / (2.0 * e);
    // river = M * xz, with M = edges_river * inverse(edges_xz); the slope over XZ is M^T * slope.
    let det = determinant(edges_xz);
    if abs(det) < 1e-8 {
        return vec3f(0.0, 1.0, 0.0);
    }
    let inverse = mat2x2f(
        vec2f(edges_xz[1].y, -edges_xz[0].y),
        vec2f(-edges_xz[1].x, edges_xz[0].x),
    ) * (1.0 / det);
    let gradient = transpose(edges_river * inverse) * slope;
    return normalize(vec3f(-gradient.x, 1.0, -gradient.y));
}

fn water_moved(xz: vec2f, river: vec2f) -> vec3f {
    return vec3f(xz.x, water_wave_height(river), xz.y);
}

@vertex
fn vs_main(vertex: VertexIn) -> WaterVertexOut {
    let instance = mesh.instances[vertex.instance];
    let own = water_moved(vertex.position.xz, vertex.uv0);
    let next = water_moved(vertex.uv1, vertex.uv3);
    let previous = water_moved(vertex.uv2, vertex.tangent.xy);
    // The triangle runs counter-clockwise seen from above, so this faces up.
    let local_normal = cross(previous - own, next - own);
    let centroid = (own + next + previous) / 3.0;
    var out: WaterVertexOut;
    out.world_position = (instance.transform * vec4f(own, 1.0)).xyz;
    out.normal = normalize((instance.transform * vec4f(local_normal, 0.0)).xyz);
    out.rest_normal = normalize((instance.transform * vec4f(0.0, 1.0, 0.0, 0.0)).xyz);
    out.centroid = (instance.transform * vec4f(centroid, 1.0)).xyz;
    out.facet = vec3f(
        vertex.normal.x,
        vertex.normal.y,
        clamp(centroid.y / max(water.wave_amplitude, 1e-4), -1.0, 1.0),
    );
    out.river = vec3f(vertex.uv0, vertex.tangent.z);
    out.footprint = vertex.bone_weights.xy;
    out.footprint_shape = vec4f(vertex.bone_weights.zw, vertex.tangent.w, vertex.normal.z);
    let local_smooth = water_smooth_normal(
        vertex.uv0,
        mat2x2f(vertex.uv1 - vertex.position.xz, vertex.uv2 - vertex.position.xz),
        mat2x2f(vertex.uv3 - vertex.uv0, vertex.tangent.xy - vertex.uv0),
    );
    out.smooth_normal = normalize((instance.transform * vec4f(local_smooth, 0.0)).xyz);
    out.position = camera.projection * camera.view * vec4f(out.world_position, 1.0);
    return out;
}
