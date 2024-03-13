struct VertexIn {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
//    @location(3) uv1: vec2<f32>,
//    @location(4) uv2: vec2<f32>,
//    @location(5) uv3: vec2<f32>,
    @location(6) bone_indices: vec4<i32>,
    @location(7) bone_weights: vec4<f32>
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
    near_plane: f32,
    far_plane: f32
};

const MAX_INSTANCES = 30u;
const MAX_BONE_INFLUENCE = 4u;
const MAX_REFLECTION_LOD = 4.0;
const PI = 3.1415926535897932384626433832795028841971693993;

struct Instance {
    bone_transform_index: i32,
    transform: mat4x4<f32>,
};

struct MeshUniforms {
    num_bones: u32,
    instances: array<Instance, MAX_INSTANCES>
};

struct BoneTransform {
    transform: mat4x4<f32>,
};

struct BoneStorage {
    bones_size: u32,
    bones: array<BoneTransform>,
};

struct AmbientLight {
    color: vec3<f32>,
    intensity: f32,
};

struct PointLight {
    position: vec3<f32>,
    radius: f32,
    color: vec3<f32>,
};

struct PointLightStorage {
    size: u32,
    lights: array<PointLight>,
};

struct DirectionalLight {
    direction: vec3<f32>,
    color: vec3<f32>,
};

struct DirectionalLightStorage {
    size: u32,
    lights: array<DirectionalLight>,
};

struct SpotLight {
    direction: vec3<f32>,
    color: vec3<f32>,
    angle: f32
};

struct MaterialProperties {
    metallic: f32,
    roughness: f32,
    ambient_occlusion: f32,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@group(0) @binding(1)
var irradiance_texture: texture_cube<f32>;

@group(0) @binding(2)
var irradiance_sampler: sampler;

@group(0) @binding(3)
var prefilter_texture: texture_cube<f32>;

@group(0) @binding(4)
var prefilter_sampler: sampler;

@group(0) @binding(5)
var brdf_texture: texture_2d<f32>;

@group(0) @binding(6)
var brdf_sampler: sampler;

@group(1) @binding(0)
var<uniform> mesh: MeshUniforms;

@group(1) @binding(1)
var<storage, read> bones: BoneStorage;

@group(2) @binding(0)
var<storage, read> point_lights: PointLightStorage;

@group(2) @binding(1)
var<storage, read> directional_lights: DirectionalLightStorage;

@group(3) @binding(0)
var texture_diffuse: texture_2d<f32>;

@group(3) @binding(1)
var sampler_diffuse: sampler;

@group(3) @binding(2)
var<uniform> material: MaterialProperties;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: VertexOut;
    if instance.bone_transform_index >= 0 {
        var total_position: vec4<f32>;
        var total_normal: vec4<f32>;
        for (var i = 0u; i < MAX_BONE_INFLUENCE; i++) {
            let bone_index = vertex.bone_indices[i];
            let bone_weight = vertex.bone_weights[i];
            if (bone_index < 0) {
                continue;
            }
            let transform_index = u32(instance.bone_transform_index) * mesh.num_bones + u32(bone_index);
            if (transform_index >= bones.bones_size) {
                continue;
            }
            let bone_transform = bones.bones[transform_index].transform;
            let local_position = bone_transform * vec4<f32>(vertex.position, 1.0);
            total_position += local_position * bone_weight;
            let local_normal = bone_transform * vec4<f32>(vertex.normal, 0.0);
            total_normal += local_normal * bone_weight;
        }
        out.world_position = (instance.transform * total_position).xyz;
        out.normal = (instance.transform * total_normal).xyz;
    } else {
        out.world_position = (instance.transform * vec4<f32>(vertex.position, 1.0)).xyz;
        out.normal = (instance.transform * vec4<f32>(vertex.normal, 0.0)).xyz;
    }
    out.position =
        camera.projection *
        camera.view *
        vec4<f32>(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

// Distribution GGX/Throwbridge-Reitz
fn d(n: vec3f, h: vec3f, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;

    let numerator = a2;
    var denominator = n_dot_h2 * (a2 - 1.0) + 1.0;
    denominator = PI * denominator * denominator;
    denominator = max(denominator, 0.000001);

    return numerator / denominator;
}

// Geometry Schlick-Beckmann (Schlick GGX)
fn g1(n: vec3f, x: vec3f, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let n_dot_x = max(dot(n, x), 0.0);

    let numerator = n_dot_x;
    var denominator = n_dot_x * (1.0 - k) + k;
    denominator = max(denominator, 0.000001);

    return numerator / denominator;
}

// Geometry Smith
fn g(n: vec3f, v: vec3f, l: vec3f, roughness: f32) -> f32 {
    return g1(n, v, roughness) * g1(n, l, roughness);
}

// Fresnel-Schlick
fn f(f0: vec3f, v: vec3f, h: vec3f, roughness: f32) -> vec3f {
    return f0 + (max(vec3f(1.0 - roughness), f0) - f0) * pow(1.0 - clamp(dot(v, h), 0.0, 1.0), 5.0);
}

fn pbr(
    n: vec3f,
    v: vec3f,
    l: vec3f,
    albedo: vec3f,
    radiance: vec3f,
    material: MaterialProperties
) -> vec3f {
    let h = normalize(v + l);
    let f0 = mix(vec3f(0.04), albedo, material.metallic);

    let ndf = d(n, h, material.roughness);
    let g = g(n, v, l, material.roughness);
    let f = f(f0, v, h, 0.0);

    let ks = f;
    let kd = (vec3f(1.0) - ks) * (1.0 - material.metallic);

    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 0.0);

    let numerator = ndf * g * f;
    var denominator = 4.0 * n_dot_v * n_dot_l;
    denominator = max(denominator, 0.000001);
    let specular = numerator / denominator;

    return (kd * albedo / PI + specular) * radiance * n_dot_l;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let albedo = 5.0 * textureSample(texture_diffuse, sampler_diffuse, in.uv);
    let n = normalize(in.normal);
    let view_position = vec3f(
        camera.inverse_view[3][0],
        camera.inverse_view[3][1],
        camera.inverse_view[3][2]
    );
    let v = normalize(view_position - in.world_position);
    let f0 = mix(vec3f(0.04), albedo.rgb, material.metallic);
    
    var color = vec3f(0.0);

    // Point lights
    for (var i = 0u; i < point_lights.size; i++) {
        let light = point_lights.lights[i];
        let to_light = light.position - in.world_position;
        let dist = length(to_light);
        if dist > light.radius {
            continue;
        }
        let l = normalize(to_light);

        let r = light.radius;
        let a = max((499.0 - r) / (r * r), 0.0);
        let attenuation = 1.0 / ((a * dist * dist) + dist + 1);

        color += pbr(n, v, l, albedo.rgb, light.color * attenuation, material);
    }

    // Directional lights
    for (var i = 0u; i < directional_lights.size; i++) {
        let light = directional_lights.lights[i];
        let l = normalize(-light.direction);
        color += pbr(n, v, l, albedo.rgb, light.color, material);
    }

    // Ambient light
    let ks = f(f0, v, n, material.roughness);
    let kd = (1.0 - ks) * (1.0 - material.metallic);
    
    let irradiance = textureSample(irradiance_texture, irradiance_sampler, n).rgb;
    let diffuse = kd * irradiance * albedo.rgb;

    let r = reflect(-v, n);
    let prefiltered_color = textureSampleLevel(
        prefilter_texture, 
        prefilter_sampler, 
        r, 
        material.roughness * MAX_REFLECTION_LOD
    ).rgb;
    let brdf = textureSample(
        brdf_texture, 
        brdf_sampler, 
        vec2f(
            max(dot(n, v), 0.0), 
            material.roughness
        )
    ).rg;
    let specular = prefiltered_color * (ks * brdf.x + brdf.y);
    
    let ambient = diffuse + specular;
    color += material.ambient_occlusion * ambient;

    // Tone mapping
    let gamma = 1.4;
    color = color / (color + vec3(1.0));
    color = pow(color, vec3(1.0 / gamma));

    return vec4f(color.xyz, albedo.a);
}

