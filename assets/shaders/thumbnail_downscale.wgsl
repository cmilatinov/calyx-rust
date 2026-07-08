@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

@group(0) @binding(2)
var output_texture: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(output_texture);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }

    let source_size = vec2f(textureDimensions(source_texture));
    let output_size = vec2f(size);
    let ratio = source_size / output_size;
    let source_lod = clamp(
        log2(max(max(ratio.x, ratio.y), 1.0)),
        0.0,
        f32(textureNumLevels(source_texture) - 1u),
    );

    var color = vec4f(0.0);
    for (var y = 0u; y < 2u; y++) {
        for (var x = 0u; x < 2u; x++) {
            let offset = (vec2f(f32(x), f32(y)) + vec2f(0.5)) * 0.5;
            let uv = (vec2f(id.xy) + offset) / output_size;
            color += textureSampleLevel(source_texture, source_sampler, uv, source_lod);
        }
    }

    textureStore(output_texture, vec2<i32>(id.xy), color * 0.25);
}
