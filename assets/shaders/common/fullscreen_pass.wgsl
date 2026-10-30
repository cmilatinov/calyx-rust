//#include "shaders/common/fullscreen.wgsl"

// The `vs_main` vertex entry point of a full-screen pass: the triangle of
// "shaders/common/fullscreen.wgsl", with its position and uv. A shader that draws the
// plain triangle includes this file and declares only its fragment stage; it
// can use `FullscreenVertexOut`, which this file includes. Draw it with
// `RenderUtils::draw_fullscreen` and pipeline options from
// `PipelineOptions::fullscreen_builder`. A shader that needs its own vertex
// stage includes "shaders/common/fullscreen.wgsl" instead.

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> FullscreenVertexOut {
    return fullscreen_vertex(index);
}
