//#include "shaders/common/fullscreen_pass.wgsl"

// Colour grading, then a vignette, on display-linear color after tone mapping.
// A disabled effect arrives as neutral values (identity white balance,
// contrast and saturation 1, vignette intensity 0), which leave the color
// unchanged. Alpha passes through.

struct ColorGradingUniform {
    // White balance in linear sRGB: a von Kries scale in CAT02 LMS, folded
    // into one matrix on the CPU.
    white_balance: mat3x3f,
    // Power around mid grey, 1 = unchanged.
    contrast: f32,
    // 0 = grey, 1 = unchanged.
    saturation: f32,
    // 0 = no vignette.
    vignette_intensity: f32,
    vignette_smoothness: f32,
    // Display-linear; alpha unused.
    vignette_color: vec4f,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var<uniform> grading: ColorGradingUniform;

// Linear 18% grey, the pivot contrast turns around.
const CONTRAST_PIVOT: f32 = 0.18;
// Rec. 709 luminance of linear sRGB.
const LUMA_WEIGHTS = vec3f(0.2126, 0.7152, 0.0722);

fn grade(color: vec3f) -> vec3f {
    var graded = max(grading.white_balance * color, vec3f(0.0));
    if grading.contrast != 1.0 {
        // A straight line through the pivot in log space.
        let ratio = max(graded, vec3f(1e-6)) / CONTRAST_PIVOT;
        graded = CONTRAST_PIVOT * exp2(grading.contrast * log2(ratio));
    }
    let luma = dot(graded, LUMA_WEIGHTS);
    graded = mix(vec3f(luma), graded, grading.saturation);
    return saturate(graded);
}

// 1 at the centre, falling towards 0 at the edges.
fn vignette_factor(position: vec2f) -> f32 {
    let size = vec2f(textureDimensions(input_texture));
    var distance = abs(position / size - 0.5) * grading.vignette_intensity * 3.0;
    // Round, not stretched to the screen's shape.
    distance.x *= size.x / size.y;
    return pow(saturate(1.0 - dot(distance, distance)), grading.vignette_smoothness * 5.0);
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let texel = textureLoad(input_texture, vec2i(in.position.xy), 0);
    let graded = grade(texel.rgb);
    let factor = vignette_factor(in.position.xy);
    let vignetted = graded * mix(grading.vignette_color.rgb, vec3f(1.0), factor);
    return vec4f(vignetted, texel.a);
}
