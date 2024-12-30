//#include "shaders/constants.wgsl"
//#include "shaders/cubemap_face.wgsl"

@group(0) @binding(0)
var src: texture_2d<f32>;

@group(0) @binding(1)
var src_sampler: sampler;

@group(0) @binding(2)
var dst: texture_storage_2d_array<rgba16float, write>;

fn sample_spherical_map(v: vec3f) -> vec2f {
    var uv = vec2f(atan2(v.z, v.x), asin(v.y));
    uv *= vec2f(0.1591, 0.3183);
    uv += vec2f(0.5);
    return uv;
}

@compute
@workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) gid: vec3u) {
    let dst_dimensions = textureDimensions(dst);
    if (gid.x >= dst_dimensions.x || gid.y >= dst_dimensions.y) {
        return;
    }
    
    let dst_dimensions_f = vec2f(dst_dimensions);
    let cube_uv = vec2f(gid.xy) / dst_dimensions_f * 2.0 - 1.0;

    let face = CUBEMAP_FACES[gid.z];
    let spherical = normalize(face.forward + face.right * cube_uv.x + face.up * cube_uv.y);
    let eq_uv = sample_spherical_map(spherical);
    let eq_pixel = vec2i(eq_uv * vec2f(textureDimensions(src)));
    
    let eq_sample = textureLoad(src, eq_pixel, 0);
    textureStore(dst, gid.xy, gid.z, eq_sample);
}