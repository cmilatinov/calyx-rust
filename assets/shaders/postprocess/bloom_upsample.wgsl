//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/bloom_filters.wgsl"

// Upsamples one bloom mip and adds it onto the next larger one: the pipeline
// blends additively, so each mip ends up holding itself plus every smaller
// mip, blurred wider the smaller it was.

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    return vec4f(bloom_upsample(input_texture, input_sampler, in.uv), 1.0);
}
