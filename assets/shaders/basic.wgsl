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

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

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


fn diffuse_lighting(
    diffuse_color: vec3<f32>, 
    light_color: vec3<f32>, 
    light_direction: vec3<f32>, 
    surface_normal: vec3<f32>,
) -> vec3<f32> {
    let radiance_factor = max(dot(surface_normal, -light_direction), 0.0);
    return diffuse_color * light_color * radiance_factor;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let diffuse_color = textureSample(texture_diffuse, sampler_diffuse, in.uv);
    let normal = normalize(in.normal);

    var color = vec3f(0.0);

    // Point lights
    for (var i = 0u; i < point_lights.size; i++) {
        let light = point_lights.lights[i];
        let from_light = in.world_position - light.position;
        let dist = length(from_light);
        let dir = normalize(from_light);

        let r = light.radius;
        let a = max((499.0 - r) / (r * r), 0.0);
        let attenuation = 1.0 / ((a * dist * dist) + dist + 1);

        color += diffuse_lighting(diffuse_color.rgb, light.color, dir, normal) * attenuation;
    }

    // Directional lights
    for (var i = 0u; i < directional_lights.size; i++) {
        let light = directional_lights.lights[i];
        color += diffuse_lighting(diffuse_color.rgb, light.color, light.direction, normal);
    }

    // Gamma correction
    // let gamma = 2.2;
    // color = pow(color, vec3f(1.0 / gamma));

    return vec4f(color.xyz, diffuse_color.a);
}
