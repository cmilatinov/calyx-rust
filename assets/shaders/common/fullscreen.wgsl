// Full-screen triangle: one triangle twice the screen's size, which the
// rasterizer clips to the screen, so it covers every pixel once. uv is [0, 1]
// on screen with (0, 0) at the top left, matching texture coordinates. Draw it
// with three vertices and no vertex buffers.
//
// A shader that only needs the plain triangle includes
// "shaders/common/fullscreen_pass.wgsl", which declares `vs_main` from this
// file. Include this file directly only for a custom vertex stage that calls
// `fullscreen_vertex(index)` and adds outputs of its own, as the grid does.

const FULLSCREEN_POSITIONS = array<vec2f, 3>(vec2f(-1.0, 1.0), vec2f(3.0, 1.0), vec2f(-1.0, -3.0));
const FULLSCREEN_UVS = array<vec2f, 3>(vec2f(0.0, 0.0), vec2f(2.0, 0.0), vec2f(0.0, 2.0));

struct FullscreenVertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

fn fullscreen_vertex(index: u32) -> FullscreenVertexOut {
    var out: FullscreenVertexOut;
    out.position = vec4f(FULLSCREEN_POSITIONS[index], 0.0, 1.0);
    out.uv = FULLSCREEN_UVS[index];
    return out;
}
