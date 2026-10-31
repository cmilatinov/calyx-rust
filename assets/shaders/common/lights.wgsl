struct PointLight {
    position: vec3f,
    radius: f32,
    color: vec3f,
    intensity: f32,
};

struct PointLightStorage {
    size: u32,
    lights: array<PointLight>,
};

@group(2) @binding(0)
var<storage, read> point_lights: PointLightStorage;

struct DirectionalLight {
    direction: vec3f,
    color: vec3f,
    intensity: f32,
};

struct DirectionalLightStorage {
    size: u32,
    lights: array<DirectionalLight>,
};

@group(2) @binding(1)
var<storage, read> directional_lights: DirectionalLightStorage;

// A light limited to a cone. Its direction is a unit vector, and the cone cosines are computed
// on the CPU, with cos_inner >= cos_outer.
struct SpotLight {
    position: vec3f,
    range: f32,
    direction: vec3f,
    cos_inner: f32,
    color: vec3f,
    intensity: f32,
    cos_outer: f32,
    _padding0: f32,
    _padding1: f32,
    _padding2: f32,
};

struct SpotLightStorage {
    size: u32,
    lights: array<SpotLight>,
};

@group(2) @binding(2)
var<storage, read> spot_lights: SpotLightStorage;

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

// The direction towards a spot light from `world_position` in xyz, and how much of the light
// reaches it in w: distance attenuation times the cone factor, 0 out of range.
fn spot_light_incidence(light: SpotLight, world_position: vec3f) -> vec4f {
    let to_light = light.position - world_position;
    let dist = length(to_light);
    if dist > light.range || dist <= 1e-6 {
        return vec4f(0.0);
    }
    let l = to_light / dist;
    let cone = spot_cone_factor(dot(-l, light.direction), light.cos_inner, light.cos_outer);
    return vec4f(l, light_distance_attenuation(dist, light.range) * cone);
}
