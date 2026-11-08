//#include "shaders/mesh/water_vertex.wgsl"

// The colour and glints of a water facet, as both the forward pass, `shaders/mesh/water.wgsl`,
// and the geometry pass, `shaders/mesh/water_gbuffer.wgsl`, shade it. Everything here is per
// facet, so each facet takes one colour.

// The facet's linear base colour: shallow at the shore running to deep over `shore_distance`,
// each facet a little lighter or darker than its neighbours, and foam within a broken line about
// `foam_width` from the shore that rises and falls with the wave.
fn water_albedo(facet: vec3f) -> vec3f {
    let shore = facet.x;
    let random = facet.y;
    let wave = facet.z;
    let depth = smoothstep(0.0, max(water.shore_distance, 1e-4), shore);
    var color = mix(water.shallow_color.rgb, water.deep_color.rgb, depth);
    color *= 1.0 + (random * 2.0 - 1.0) * water.facet_variation;
    let foam_edge = water.foam_width * (0.35 + 1.1 * fract(random * 7.31)) * (1.0 + 0.25 * wave);
    if shore < foam_edge {
        color = water.foam_color.rgb;
    }
    return max(color, vec3f(0.0));
}

// The glint of the light on a facet, as HDR emission: a facet glints once it has turned from the
// rest surface most of the way towards mirroring the light into the camera. A flat surface seen
// from a high camera rarely mirrors a sun that is not behind the camera, so the glints are the
// facets turned furthest towards it, and they move with the wave.
fn water_glint(in: WaterVertexOut) -> vec3f {
    let to_light = -normalize(water.light_direction.xyz);
    let camera_position = camera.inverse_view[3].xyz;
    let to_camera = normalize(camera_position - in.centroid);
    let aligned = dot(reflect(-to_camera, in.normal), to_light);
    let at_rest = dot(reflect(-to_camera, in.rest_normal), to_light);
    let turned = (aligned - at_rest) / max(1.0 - at_rest, 1e-3);
    let lit = dot(in.normal, to_light) > 0.0 && dot(in.rest_normal, to_light) > 0.0;
    return water.glint_color.rgb * smoothstep(0.3, 0.65, turned) * select(0.0, 1.0, lit);
}
