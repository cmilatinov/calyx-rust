//#include "shaders/fullscreen_pass.wgsl"
//#include "shaders/postprocess/ssao_common.wgsl"

// Multiplies the blurred occlusion into the HDR scene color at full
// resolution. The half-resolution occlusion is upsampled from its four
// nearest texels, weighted bilinearly and by how close their view depth is
// to this pixel's, so it does not bleed across silhouettes. Sky pixels and
// alpha pass through untouched.

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var occlusion_texture: texture_2d<f32>;

@group(0) @binding(2)
var depth_texture: texture_depth_2d;

@group(0) @binding(3)
var<uniform> ssao: SsaoUniform;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    let depth = textureLoad(depth_texture, pixel, 0);
    if ssao_is_sky(depth) {
        return texel;
    }
    let z = ssao_linear_depth(ssao.near, ssao.far, depth);

    let half_size = vec2i(textureDimensions(occlusion_texture));
    // Occlusion texel (i, j) was computed at full-resolution pixel (2i, 2j).
    let coord = in.position.xy * 0.5 - 0.25;
    let base = vec2i(floor(coord));
    let f = coord - floor(coord);
    var sum = 0.0;
    var weights = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let at = clamp(base + vec2i(x, y), vec2i(0), half_size - 1);
            let occlusion = textureLoad(occlusion_texture, at, 0);
            let bilinear = mix(1.0 - f.x, f.x, f32(x)) * mix(1.0 - f.y, f.y, f32(y));
            let weight = bilinear * ssao_depth_weight(occlusion.g, z) + 1e-5;
            sum += occlusion.r * weight;
            weights += weight;
        }
    }
    let visibility = sum / weights;
    return vec4f(texel.rgb * visibility, texel.a);
}
