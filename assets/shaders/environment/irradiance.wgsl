//#include "shaders/constants.wgsl"
//#include "shaders/cubemap_face.wgsl"

@group(0) @binding(0)
var src: texture_cube<f32>;

@group(0) @binding(1)
var src_sampler: sampler;

@group(0) @binding(2)
var dst: texture_storage_2d_array<rgba16float, write>;

@compute
@workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) gid: vec3u) {
    let dst_dimensions = textureDimensions(dst);
    if (gid.x >= dst_dimensions.x || gid.y >= dst_dimensions.y) {
        return;
    }
    
    let face = CUBEMAP_FACES[gid.z];
    let dst_dimensions_f = vec2f(dst_dimensions);
    let cube_uv = vec2f(gid.xy) / dst_dimensions_f * 2.0 - 1.0;
    let normal = normalize(face.forward + face.right * cube_uv.x + face.up * cube_uv.y);
    var irradiance = vec3f(0.0);
    var up = vec3f(0.0, 1.0, 0.0);
    let right = normalize(cross(up, normal));
    up = normalize(cross(normal, right));
    
    let delta = 0.025;
    var num_samples = 0u;
    for (var phi = 0.0; phi < 2.0 * PI; phi += delta) {
        for (var theta = 0.0; theta < 0.5 * PI; theta += delta) {
            let tangent_sample = vec3f(
                sin(theta) * cos(phi), 
                sin(theta) * sin(phi), 
                cos(theta)
            );
            let uvw = normalize(
                tangent_sample.x * right + 
                tangent_sample.y * up + 
                tangent_sample.z * normal
            );
            irradiance += 
                clamp(textureSampleLevel(src, src_sampler, uvw, 0.0).rgb, vec3f(0.0), vec3f(50.0)) * 
                cos(theta) * 
                sin(theta);
            num_samples++;
        }
    }
    irradiance = PI * irradiance * (1.0 / f32(num_samples));
    textureStore(
        dst,
        gid.xy,
        gid.z,
        vec4f(irradiance, 1.0)
    );
}