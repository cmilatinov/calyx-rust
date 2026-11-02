//#include "shaders/common/constants.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/common/lights.wgsl"
//#include "shaders/common/shadows.wgsl"
//#include "shaders/common/pbr_utils.wgsl"

// The PBR lighting of one shaded point, shared by the forward PBR pass, `shaders/mesh/pbr.wgsl`,
// and the deferred lighting pass, `shaders/deferred/lighting_common.wgsl`, so both shade a
// surface with the same direct, shadow, sky and ambient terms over the same bindings:
// - group 0: the scene camera (binding 0, `shaders/common/camera.wgsl`), then the sky light's
//   irradiance, prefiltered environment and BRDF maps and the environment uniform (bindings 1
//   to 7, below);
// - group 2: the lights (binding 0, `shaders/common/lights.wgsl`) and the shadow maps (bindings
//   1 to 4, `shaders/common/shadows.wgsl`).
// Each pass binds the same bind groups for both groups.

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

// What the lighting needs of a shaded point. Directions are world space and unit length.
struct PbrShadingPoint {
    world_position: vec3f,
    // Linear base color.
    albedo: vec3f,
    // The shading normal, which the lighting uses.
    normal: vec3f,
    // The geometric normal, which shadow lookups offset along.
    geometric_normal: vec3f,
    metallic: f32,
    roughness: f32,
    // Multiplies the ambient and image-based light only.
    occlusion: f32,
    // Linear HDR radiance added on top.
    emission: vec3f,
};

// The linear HDR radiance leaving `point` towards the camera: every light's direct term, dimmed
// by its shadow, then the sky light and the flat ambient term, dimmed by the occlusion, then
// the emission. Every texture is sampled at an explicit level, so a pass may call it in
// non-uniform control flow; the irradiance and BRDF maps have a single level.
fn pbr_lighting(point: PbrShadingPoint) -> vec3f {
    let albedo = point.albedo;
    let n = point.normal;
    let metallic = point.metallic;
    let roughness = point.roughness;
    let view_position = vec3f(
        camera.inverse_view[3][0],
        camera.inverse_view[3][1],
        camera.inverse_view[3][2]
    );
    let v = normalize(view_position - point.world_position);
    let f0 = mix(vec3f(0.04), albedo, metallic);

    var color = vec3f(0.0);

    // The lights are grouped by kind (see `LightStorage`): one loop per kind over its range.
    let directional_end = light_storage.directional_count;
    let point_end = directional_end + light_storage.point_count;
    let spot_end = point_end + light_storage.spot_count;

    // Shadows dim each light's direct term only, never the ambient and image-based light. They
    // are looked up with the geometric normal, which the normal offset follows.
    let geometric_normal = point.geometric_normal;

    for (var i = 0u; i < directional_end; i++) {
        let light = light_storage.lights[i];
        let incidence = directional_radiance(light);
        let shadow = directional_shadow(light, point.world_position, geometric_normal);
        color += shadow *
            pbr(n, v, incidence.l, albedo, incidence.radiance, metallic, roughness);
    }

    for (var i = directional_end; i < point_end; i++) {
        let incidence = point_radiance(light_storage.lights[i], point.world_position);
        color += pbr(n, v, incidence.l, albedo, incidence.radiance, metallic, roughness);
    }

    for (var i = point_end; i < spot_end; i++) {
        let light = light_storage.lights[i];
        let incidence = spot_radiance(light, point.world_position);
        let shadow = spot_shadow(light, point.world_position, geometric_normal);
        color += shadow *
            pbr(n, v, incidence.l, albedo, incidence.radiance, metallic, roughness);
    }

    // Ambient light
    let ks = f(f0, v, n, roughness);
    let kd = (1.0 - ks) * (1.0 - metallic);

    let irradiance = environment.sky_light.x *
        textureSampleLevel(irradiance_texture, irradiance_sampler, n, 0.0).rgb;
    let diffuse = kd * irradiance * albedo;

    let r = reflect(-v, n);
    let prefiltered_color =
        environment.sky_light.x *
        textureSampleLevel(
            prefilter_texture,
            prefilter_sampler,
            r,
            roughness * f32(textureNumLevels(prefilter_texture) - 1u)
        ).rgb;
    let brdf = textureSampleLevel(
        brdf_texture,
        brdf_sampler,
        vec2f(
            max(dot(n, v), 0.0),
            roughness
        ),
        0.0
    ).rg;
    let specular = prefiltered_color * (ks * brdf.x + brdf.y);

    let ambient = diffuse + specular;
    let direct_ambient = kd * environment.ambient_light.rgb * albedo;
    color += point.occlusion * (ambient + direct_ambient);
    color += point.emission;
    return color;
}
