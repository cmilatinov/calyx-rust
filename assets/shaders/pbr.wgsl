//#include "shaders/constants.wgsl"
//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"
//#include "shaders/mesh.wgsl"
//#include "shaders/lights.wgsl"
//#include "shaders/pbr_utils.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
};

struct MaterialProperties {
    metallic: f32,
    roughness: f32,
    ambient_occlusion: f32,
};

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
        var total_position: vec4f;
        var total_normal: vec4f;
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
            let local_position = bone_transform * vec4f(vertex.position, 1.0);
            total_position += local_position * bone_weight;
            let local_normal = bone_transform * vec4f(vertex.normal, 0.0);
            total_normal += local_normal * bone_weight;
        }
        out.world_position = (instance.transform * total_position).xyz;
        out.normal = (instance.transform * total_normal).xyz;
    } else {
        out.world_position = (instance.transform * vec4f(vertex.position, 1.0)).xyz;
        out.normal = (instance.transform * vec4f(vertex.normal, 0.0)).xyz;
    }
    out.position =
        camera.projection *
        camera.view *
        vec4f(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
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
        let a = max((999.0 - r) / (r * r), 0.0);
        let attenuation = 1.0 / ((a * dist * dist) + dist + 1.0);

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
        material.roughness * f32(textureNumLevels(prefilter_texture) - 1u)
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

