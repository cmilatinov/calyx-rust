//#include "shaders/deferred/lighting_common.wgsl"

// The deferred lighting pass at one sample: writes the lit color of every pixel the geometry
// pass covered, which drew into the scene depth itself.

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = deferred_shade(in.position.xy);
    if !pixel.covered {
        discard;
    }
    return pixel.color;
}
