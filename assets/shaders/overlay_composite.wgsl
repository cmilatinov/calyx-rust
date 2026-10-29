//#include "shaders/fullscreen_pass.wgsl"

// Composites the resolved overlay target over the post-processed scene
// texture. Overlays alpha blend into a target cleared to transparent, which
// leaves premultiplied color with coverage alpha, so the pipeline blends the
// texel as premultiplied alpha: rgb + dst * (1 - a). Multiplying the color by
// alpha again would darken anti-aliased edges.

@group(0) @binding(0)
var overlay: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    return textureLoad(overlay, vec2i(in.position.xy), 0);
}
