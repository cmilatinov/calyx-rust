//#include "shaders/mesh/water_vertex.wgsl"

// The colour, foam and glints of the water, as both the forward pass, `shaders/mesh/water.wgsl`,
// and the geometry pass, `shaders/mesh/water_gbuffer.wgsl`, shade it.

// Whether the water is shaded smooth rather than faceted.
fn water_is_smooth() -> bool {
    return water.shading == 1u;
}

// The shading normal: the facet's own when faceted, the waves' interpolated one when smooth.
fn water_normal(in: WaterVertexOut) -> vec3f {
    return select(in.normal, normalize(in.smooth_normal), water_is_smooth());
}

// The water's linear base colour, shallow at the shore running to deep over `shore_distance`.
// Faceted, it is one colour per facet, from the facet's centroid, each facet a little lighter or
// darker than its neighbours; smooth, it follows each point's distance to the shore.
fn water_base_color(in: WaterVertexOut) -> vec3f {
    let is_smooth = water_is_smooth();
    let shore = select(in.facet.y, in.river.z, is_smooth);
    let depth = smoothstep(0.0, max(water.shore_distance, 1e-4), shore);
    var color = mix(water.shallow_color.rgb, water.deep_color.rgb, depth);
    let variation = select(water.facet_variation, 0.0, is_smooth);
    color *= 1.0 + (in.facet.x * 2.0 - 1.0) * variation;
    return max(color, vec3f(0.0));
}

// Whether there is foam at a point with river coordinates and distances `river` (s, t, distance
// to the shore, distance to the nearest obstacle): 1 or 0, so the foam has hard edges, in the
// flat-colour style. All of it drifts downstream with the flow:
// - a band along both shores, about `foam_width` wide, whose inner edge swells and breaks;
// - loose flecks just past the band;
// - patches in the open water, stretched along the flow, covering about `foam_patches` of it;
// - a thin, broken ring about `obstacle_foam` wide around each obstacle.
fn water_foam(river: vec4f) -> f32 {
    let s = river.x;
    let t = river.y;
    let shore = river.z;
    let obstacle = river.w;
    let drift = water.flow_speed * water.time;
    let edge = water_noise(vec2f(s * 0.9 - drift * 0.9, t * 0.4 + water.time * 0.15));
    let band = water.foam_width * (0.35 + 1.0 * edge);
    var foam = step(shore, band);
    let fleck = water_noise(vec2f(s * 2.4 - drift * 1.2, t * 2.4));
    foam = max(foam, step(shore, water.foam_width * 2.2) * step(0.8, fleck));
    let patch_shape = 0.65 * water_noise(vec2f(s * 0.3 - drift * 0.3, t * 0.9 + 17.0))
        + 0.35 * water_noise(vec2f(s * 1.1 - drift * 1.1, t * 2.2 + 5.0));
    let patch_threshold = mix(1.01, 0.55, clamp(water.foam_patches, 0.0, 1.0));
    foam = max(foam, step(patch_threshold, patch_shape));
    // The ring hugs the obstacle: a thin band whose outer edge swells with the flow, broken
    // into pieces that drift downstream.
    let swell = water_noise(vec2f(s * 1.8 - drift * 1.6, t * 1.8 + 31.0));
    let pieces = water_noise(vec2f(s * 3.0 - drift * 1.4, t * 3.0 + 11.0));
    let ring = water.obstacle_foam * (0.3 + 0.9 * swell);
    foam = max(foam, step(obstacle, ring) * step(0.35, pieces));
    return foam;
}

// The point's linear base colour: the water's, or foam.
fn water_albedo(in: WaterVertexOut) -> vec3f {
    return mix(water_base_color(in), water.foam_color.rgb, water_foam(in.river));
}

// The glint of the light on the water, as HDR emission, where the waves turn from rest most of the
// way towards mirroring the light into the camera. A flat surface seen from a high camera rarely
// mirrors a sun that is not behind the camera, so the glints are where the waves turn furthest
// towards it, and they come and go as the waves roll. Faceted, whole facets flash, judged at
// their centroids. Smooth, the light sparkles: small round points scattered over the turned
// slopes, twinkling as they drift with the flow.
fn water_glint(in: WaterVertexOut) -> vec3f {
    let is_smooth = water_is_smooth();
    let normal = water_normal(in);
    let point = select(in.centroid, in.world_position, is_smooth);
    let to_light = -normalize(water.light_direction.xyz);
    let camera_position = camera.inverse_view[3].xyz;
    let to_camera = normalize(camera_position - point);
    let aligned = dot(reflect(-to_camera, normal), to_light);
    let at_rest = dot(reflect(-to_camera, in.rest_normal), to_light);
    let turned = (aligned - at_rest) / max(1.0 - at_rest, 1e-3);
    let lit = dot(normal, to_light) > 0.0 && dot(in.rest_normal, to_light) > 0.0;
    var amount = smoothstep(0.5, 0.8, turned);
    if is_smooth {
        // Sparkle points about 0.3 units across on a jittered lattice, each lit for part of
        // its own cycle, shown where the slope turns towards the light.
        let drift = water.flow_speed * water.time;
        let p = vec2f(in.river.x - drift, in.river.y) * 2.2;
        let cell = floor(p);
        let jitter = vec2f(water_hash(cell + 0.37), water_hash(cell + 7.11));
        let distance = length(fract(p) - (0.25 + 0.5 * jitter));
        let twinkle = fract(water_hash(cell + 3.3) + water.time * 0.6);
        let point_on = step(distance, 0.17) * step(0.65, twinkle);
        amount = smoothstep(0.25, 0.6, turned) * point_on;
    }
    return water.glint_color.rgb * amount * select(0.0, 1.0, lit);
}
