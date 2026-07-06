@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var output_texture: texture_storage_2d<rgba8unorm, write>;

fn sample_spherical_map(direction: vec3f) -> vec2f {
    var uv = vec2f(atan2(direction.z, direction.x), asin(direction.y));
    uv *= vec2f(0.1591, 0.3183);
    uv += vec2f(0.5);
    return uv;
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let output_size = textureDimensions(output_texture);
    if (id.x >= output_size.x || id.y >= output_size.y) {
        return;
    }

    let cube_uv = (vec2f(id.xy) + vec2f(0.5, 0.5)) / vec2f(output_size) * 2.0 - 1.0;
    let direction = normalize(vec3f(cube_uv.x, cube_uv.y, 1.0));
    let source_uv = sample_spherical_map(direction);

    let source_size = textureDimensions(source_texture);
    let source_max = vec2f(source_size - vec2u(1u, 1u));
    let source_pixel = vec2i(clamp(source_uv * vec2f(source_size), vec2f(0.0, 0.0), source_max));
    let color = textureLoad(source_texture, source_pixel, 0);

    textureStore(output_texture, vec2i(id.xy), vec4f(color.rgb, 1.0));
}
