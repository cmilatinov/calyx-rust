//#include "shaders/fullscreen.wgsl"

// Copies sample 0 of the multisampled scene depth into a single-sample depth
// target, for post-processing passes that read depth.

@group(0) @binding(0)
var depth_msaa: texture_depth_multisampled_2d;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> FullscreenVertexOut {
    return fullscreen_vertex(index);
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @builtin(frag_depth) f32 {
    return textureLoad(depth_msaa, vec2i(in.position.xy), 0);
}
