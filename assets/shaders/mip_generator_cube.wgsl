@group(0) @binding(0) 
var src: texture_2d_array<f32>;

@group(0) @binding(1)
var dst: texture_storage_2d_array<rgba16float, write>;

@compute
@workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) gid: vec3u) {
    let dimensions = textureDimensions(dst);
    if (gid.x >= dimensions.x || gid.y >= dimensions.y) {
        return;
    }
    let src_coord = gid.xy * 2u;
    let face = gid.z;
    let c00 = textureLoad(src, src_coord + vec2u(0u, 0u), face, 0);
    let c10 = textureLoad(src, src_coord + vec2u(1u, 0u), face, 0);
    let c01 = textureLoad(src, src_coord + vec2u(0u, 1u), face, 0);
    let c11 = textureLoad(src, src_coord + vec2u(1u, 1u), face, 0);
    let avg_color = (c00 + c10 + c01 + c11) * 0.25;
    textureStore(dst, gid.xy, face, avg_color);
}