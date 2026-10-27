// Full-screen triangle shared by the post-processing passes. Draw it with
// three vertices and no vertex buffers; it covers the target with one
// triangle, so no pixel is shaded twice along a diagonal.

struct FullscreenVertexOut {
    @builtin(position) position: vec4f,
    // 0..1 across the target, with (0, 0) at the top left.
    @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> FullscreenVertexOut {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    var out: FullscreenVertexOut;
    out.position = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}
