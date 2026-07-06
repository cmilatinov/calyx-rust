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

    let uv = (vec2<f32>(id.xy) + vec2<f32>(0.5, 0.5)) / vec2<f32>(size);
    let color = textureSampleLevel(source_texture, source_sampler, uv, 0.0);
    textureStore(output_texture, vec2<i32>(id.xy), color);
}
