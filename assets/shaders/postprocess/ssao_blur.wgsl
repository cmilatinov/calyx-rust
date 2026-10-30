//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/ssao_common.wgsl"

// Depth-aware 4x4 blur of the half-resolution occlusion. The 4x4 window
// covers one full tile of the noise pattern, so it cancels the rotation
// noise, and texels whose view depth differs from the center's are left
// out, so occlusion does not bleed across silhouettes. View depth passes
// through in green.

@group(0) @binding(0)
var occlusion_texture: texture_2d<f32>;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let size = vec2i(textureDimensions(occlusion_texture));
    let center = textureLoad(occlusion_texture, pixel, 0);
    var sum = 0.0;
    var weights = 0.0;
    for (var y = -2; y < 2; y++) {
        for (var x = -2; x < 2; x++) {
            let texel = textureLoad(
                occlusion_texture,
                clamp(pixel + vec2i(x, y), vec2i(0), size - 1),
                0,
            );
            let weight = ssao_depth_weight(texel.g, center.g);
            sum += texel.r * weight;
            weights += weight;
        }
    }
    // The center texel always weighs 1 when it is inside the window.
    let visibility = select(center.r, sum / weights, weights > 0.0);
    return vec4f(visibility, center.g, 0.0, 1.0);
}
