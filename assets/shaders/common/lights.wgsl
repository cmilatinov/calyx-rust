// Every enabled light, as `LightManager` uploads it. One struct carries every kind; each kind
// reads only the fields it uses, and the CPU zeroes the rest:
// - directional: direction, color, intensity
// - point: position, range, color, intensity
// - spot: position, range, direction, cos_inner, cos_outer, color, intensity
// A direction is a unit vector the light travels along. A spot light's cone cosines are
// computed on the CPU, with cos_inner >= cos_outer.
struct Light {
    position: vec3f,
    range: f32,
    direction: vec3f,
    cos_inner: f32,
    color: vec3f,
    intensity: f32,
    cos_outer: f32,
    // The light's shadow, or -1 for none: its cascades or atlas tile in `shaders/common/shadows.wgsl`.
    shadow_index: i32,
    _pad: vec2f,
};

// The lights grouped by kind in contiguous ranges, in this order: `directional_count`
// directional lights, then `point_count` point lights, then `spot_count` spot lights. The range
// a light sits in is its kind, so a light carries no kind field and each kind's loop is
// branch-free over its own range.
struct LightStorage {
    directional_count: u32,
    point_count: u32,
    spot_count: u32,
    lights: array<Light>,
};

@group(2) @binding(0)
var<storage, read> light_storage: LightStorage;

// The direction towards a light from the shaded point, and the radiance reaching it, zero when
// none does.
struct LightIncidence {
    l: vec3f,
    radiance: vec3f,
};

// Distance attenuation of a point or spot light `dist` away, within its range `r`.
fn light_distance_attenuation(dist: f32, r: f32) -> f32 {
    let a = max((999.0 - r) / (r * r), 0.0);
    return 1.0 / ((a * dist * dist) + dist + 1.0);
}

// How much of a spot light reaches a point whose direction from the light makes the cosine
// `cos_angle` with the cone axis: 1 inside the inner cone, 0 outside the outer cone, and a
// smooth falloff between them. Equal cones give a hard edge instead of dividing by zero.
fn spot_cone_factor(cos_angle: f32, cos_inner: f32, cos_outer: f32) -> f32 {
    if cos_inner - cos_outer <= 1e-6 {
        return select(0.0, 1.0, cos_angle >= cos_outer);
    }
    return smoothstep(cos_outer, cos_inner, cos_angle);
}

fn directional_radiance(light: Light) -> LightIncidence {
    return LightIncidence(normalize(-light.direction), light.color * light.intensity);
}

// A point light reaches `world_position` within its range, attenuated by distance.
fn point_radiance(light: Light, world_position: vec3f) -> LightIncidence {
    let to_light = light.position - world_position;
    let dist = length(to_light);
    if dist > light.range || dist <= 1e-6 {
        return LightIncidence(vec3f(0.0), vec3f(0.0));
    }
    let attenuation = light_distance_attenuation(dist, light.range);
    return LightIncidence(to_light / dist, light.color * light.intensity * attenuation);
}

// A spot light is a point light limited to its cone.
fn spot_radiance(light: Light, world_position: vec3f) -> LightIncidence {
    var incidence = point_radiance(light, world_position);
    incidence.radiance *=
        spot_cone_factor(dot(-incidence.l, light.direction), light.cos_inner, light.cos_outer);
    return incidence;
}
