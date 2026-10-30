@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

@group(0) @binding(2)
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

    var color = vec4f(0.0);
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let offset = (vec2f(f32(x), f32(y)) + vec2f(0.5)) * 0.5;
            let cube_uv = (vec2f(id.xy) + offset) / vec2f(output_size) * 2.0 - 1.0;
            let direction = normalize(vec3f(cube_uv.x, cube_uv.y, 1.0));
            let source_uv = sample_spherical_map(direction);
            color += textureSampleLevel(source_texture, source_sampler, source_uv, 0.0);
        }
    }

    textureStore(output_texture, vec2i(id.xy), vec4f((color * 0.25).rgb, 1.0));
}
