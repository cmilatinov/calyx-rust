//#include "shaders/fullscreen_pass.wgsl"
//#include "shaders/postprocess/bloom_filters.wgsl"

// Downsamples one bloom mip into the next, half its size.

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    return vec4f(bloom_downsample(input_texture, input_sampler, in.uv), 1.0);
}
