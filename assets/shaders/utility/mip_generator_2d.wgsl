//#include "shaders/common/fullscreen_pass.wgsl"

@group(0) @binding(0)
var in_texture: texture_2d<f32>;

@group(0) @binding(1)
var in_sampler: sampler;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    return textureSample(in_texture, in_sampler, in.uv);
}
