//#include "shaders/deferred/lighting_common.wgsl"

// The deferred lighting pass above one sample: writes the lit color of every pixel the
// geometry pass covered into every sample of the multisampled scene target, and the
// G-buffer's single-sample depth into every sample of the scene depth, so the forward draws,
// the skybox and the particles that follow test against the deferred surfaces.

struct DeferredMsaaOut {
    @location(0) color: vec4f,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_main(in: FullscreenVertexOut) -> DeferredMsaaOut {
    let pixel = deferred_shade(in.position.xy);
    if !pixel.covered {
        discard;
    }
    return DeferredMsaaOut(pixel.color, pixel.depth);
}
