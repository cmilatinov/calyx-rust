struct PointLight {
    position: vec3f,
    radius: f32,
    color: vec3f,
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
};

struct DirectionalLightStorage {
    size: u32,
    lights: array<DirectionalLight>,
};

@group(2) @binding(1)
var<storage, read> directional_lights: DirectionalLightStorage;
