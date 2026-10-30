//#include "shaders/common/fullscreen_pass.wgsl"

// Composites the resolved overlay target over the post-processed scene
// texture. Overlays alpha blend into a target cleared to transparent, which
// leaves premultiplied color with coverage alpha, so the pipeline blends the
// texel as premultiplied alpha: rgb + dst * (1 - a). Multiplying the color by
// alpha again would darken anti-aliased edges.
//
// The texel is held to that contract here: coverage in [0, 1] and color no
// brighter than it. Coverage above one would weight the scene by a negative
// factor, so the overlay would subtract the post-processed scene and change
// with its exposure and tone mapping.

@group(0) @binding(0)
var overlay: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let texel = textureLoad(overlay, vec2i(in.position.xy), 0);
    let coverage = clamp(texel.a, 0.0, 1.0);
    return vec4f(clamp(texel.rgb, vec3f(0.0), vec3f(coverage)), coverage);
}
