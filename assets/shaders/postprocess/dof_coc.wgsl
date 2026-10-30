//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/dof_common.wgsl"

// Depth of field, first step: the signed circle of confusion of every pixel,
// in pixels. Negative is in front of the focus plane (near field), positive
// behind it (far field), zero on it.

@group(0) @binding(0)
var depth_texture: texture_depth_2d;

@group(0) @binding(1)
var<uniform> dof: DofUniform;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let depth = textureLoad(depth_texture, vec2i(in.position.xy), 0);
    return vec4f(circle_of_confusion(dof, depth), 0.0, 0.0, 1.0);
}
