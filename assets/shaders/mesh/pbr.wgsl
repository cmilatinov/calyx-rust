//#include "shaders/common/constants.wgsl"
//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"
//#include "shaders/mesh/pbr_material.wgsl"
//#include "shaders/common/lights.wgsl"
//#include "shaders/common/shadows.wgsl"
//#include "shaders/common/pbr_utils.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    // World-space tangent along increasing U; w is the bitangent's handedness.
    @location(3) tangent: vec4f,
};

struct EnvironmentProperties {
    sky_light: vec4f,
    ambient_light: vec4f,
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

@group(0) @binding(7)
var<uniform> environment: EnvironmentProperties;

// Material textures. Each sampler samples the texture bound just before it. The color
// textures, base color and emissive, sample as sRGB, and the others as linear (see `Material`).

@group(3) @binding(0)
var base_color_texture: texture_2d<f32>;

@group(3) @binding(1)
var base_color_sampler: sampler;

@group(3) @binding(2)
var<uniform> material: MaterialProperties;

// glTF packing: roughness in G, metallic in B.
@group(3) @binding(3)
var metallic_roughness_texture: texture_2d<f32>;

@group(3) @binding(4)
var metallic_roughness_sampler: sampler;

// Tangent-space normals, +Y up the image. Defaults to the flat normal.
@group(3) @binding(5)
var normal_texture: texture_2d<f32>;

@group(3) @binding(6)
var normal_sampler: sampler;

@group(3) @binding(7)
var emissive_texture: texture_2d<f32>;

@group(3) @binding(8)
var emissive_sampler: sampler;

// Ambient occlusion in R, so one texture can hold occlusion, roughness and metallic.
@group(3) @binding(9)
var occlusion_texture: texture_2d<f32>;

@group(3) @binding(10)
var occlusion_sampler: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: VertexOut;
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    let local_normal = skinned_local(vertex, instance, vec4f(vertex.normal, 0.0));
    let local_tangent = skinned_local(vertex, instance, vec4f(vertex.tangent.xyz, 0.0));
    out.world_position = (instance.transform * local_position).xyz;
    out.normal = (instance.transform * local_normal).xyz;
    out.tangent = vec4f((instance.transform * local_tangent).xyz, vertex.tangent.w);
    out.position =
        camera.projection *
        camera.view *
        vec4f(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

// The shading normal: the vertex normal, bent by the normal texture in the tangent frame.
// `tangent_in` is the interpolated vertex tangent and `sampled` the normal texture's texel.
fn shading_normal(normal: vec3f, tangent_in: vec4f, sampled: vec3f) -> vec3f {
    let n = normalize(normal);
    let tangent = tangent_in.xyz - n * dot(n, tangent_in.xyz);
    let tangent_length = length(tangent);
    if tangent_length < 1e-6 {
        return n;
    }
    let t = tangent / tangent_length;
    let b = cross(n, t) * select(1.0, -1.0, tangent_in.w < 0.0);
    let tangent_normal =
        (sampled * 2.0 - 1.0) * vec3f(material.normal_scale, material.normal_scale, 1.0);
    let bent = mat3x3f(t, b, n) * tangent_normal;
    if dot(bent, bent) < 1e-12 {
        return n;
    }
    return normalize(bent);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    // Every texture is sampled up front: sampling needs uniform control flow.
    let albedo =
        textureSample(base_color_texture, base_color_sampler, in.uv) * material.base_color_factor;
    let metallic_roughness =
        textureSample(metallic_roughness_texture, metallic_roughness_sampler, in.uv);
    let normal_sample = textureSample(normal_texture, normal_sampler, in.uv).xyz;
    let emissive = textureSample(emissive_texture, emissive_sampler, in.uv).rgb;
    let occlusion_sample = textureSample(occlusion_texture, occlusion_sampler, in.uv).r;
    if albedo.a < material.alpha_cutoff {
        discard;
    }
    let n = shading_normal(in.normal, in.tangent, normal_sample);
    let metallic = material.metallic * metallic_roughness.b;
    let roughness = material.roughness * metallic_roughness.g;
    let occlusion = material.ambient_occlusion *
        (1.0 + material.occlusion_strength * (occlusion_sample - 1.0));
    let view_position = vec3f(
        camera.inverse_view[3][0],
        camera.inverse_view[3][1],
        camera.inverse_view[3][2]
    );
    let v = normalize(view_position - in.world_position);
    let f0 = mix(vec3f(0.04), albedo.rgb, metallic);

    var color = vec3f(0.0);

    // The lights are grouped by kind (see `LightStorage`): one loop per kind over its range.
    let directional_end = light_storage.directional_count;
    let point_end = directional_end + light_storage.point_count;
    let spot_end = point_end + light_storage.spot_count;

    // Shadows dim each light's direct term only, never the ambient and image-based light. They
    // are looked up with the geometric normal, which the normal offset follows.
    let geometric_normal = normalize(in.normal);

    for (var i = 0u; i < directional_end; i++) {
        let light = light_storage.lights[i];
        let incidence = directional_radiance(light);
        let shadow = directional_shadow(light, in.world_position, geometric_normal);
        color += shadow *
            pbr(n, v, incidence.l, albedo.rgb, incidence.radiance, metallic, roughness);
    }

    for (var i = directional_end; i < point_end; i++) {
        let incidence = point_radiance(light_storage.lights[i], in.world_position);
        color += pbr(n, v, incidence.l, albedo.rgb, incidence.radiance, metallic, roughness);
    }

    for (var i = point_end; i < spot_end; i++) {
        let light = light_storage.lights[i];
        let incidence = spot_radiance(light, in.world_position);
        let shadow = spot_shadow(light, in.world_position, geometric_normal);
        color += shadow *
            pbr(n, v, incidence.l, albedo.rgb, incidence.radiance, metallic, roughness);
    }

    // Ambient light
    let ks = f(f0, v, n, roughness);
    let kd = (1.0 - ks) * (1.0 - metallic);

    let irradiance =
        environment.sky_light.x * textureSample(irradiance_texture, irradiance_sampler, n).rgb;
    let diffuse = kd * irradiance * albedo.rgb;

    let r = reflect(-v, n);
    let prefiltered_color =
        environment.sky_light.x *
        textureSampleLevel(
            prefilter_texture,
            prefilter_sampler,
            r,
            roughness * f32(textureNumLevels(prefilter_texture) - 1u)
        ).rgb;
    let brdf = textureSample(
        brdf_texture,
        brdf_sampler,
        vec2f(
            max(dot(n, v), 0.0),
            roughness
        )
    ).rg;
    let specular = prefiltered_color * (ks * brdf.x + brdf.y);

    let ambient = diffuse + specular;
    let direct_ambient = kd * environment.ambient_light.rgb * albedo.rgb;
    color += occlusion * (ambient + direct_ambient);
    color += material.emission_color.rgb * material.emission_strength * emissive;

    // Linear HDR radiance: the post-processing chain tone maps it.
    return vec4f(color.xyz, albedo.a);
}

