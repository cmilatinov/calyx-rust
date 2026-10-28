//#include "shaders/fullscreen.wgsl"

@group(0) @binding(0)
var in_texture: texture_2d<f32>;

@group(0) @binding(1)
var in_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> FullscreenVertexOut {
    return fullscreen_vertex(index);
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    return textureSample(in_texture, in_sampler, in.uv);
}
