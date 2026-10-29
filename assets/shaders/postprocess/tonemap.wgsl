//#include "shaders/fullscreen_pass.wgsl"

// Maps linear HDR scene color to display-linear color in [0, 1]. The scene
// texture holds display-linear color: egui and the readback sRGB-encode it
// themselves, so no gamma is applied here.

struct TonemapUniform {
    // 2^EV, applied before the curve.
    exposure_scale: f32,
    // 0 = ACES.
    preset: u32,
    _padding: vec2u,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var<uniform> tonemap: TonemapUniform;

// ACES fit by Stephen Hill (@self_shadow), from BakingLab (MIT license):
// https://github.com/TheRealMJP/BakingLab/blob/master/BakingLab/ACES.hlsl
// sRGB => XYZ => D65_2_D60 => AP1 => RRT_SAT
const ACES_INPUT = mat3x3f(
    vec3f(0.59719, 0.07600, 0.02840),
    vec3f(0.35458, 0.90834, 0.13383),
    vec3f(0.04823, 0.01566, 0.83777),
);

// ODT_SAT => XYZ => D60_2_D65 => sRGB
const ACES_OUTPUT = mat3x3f(
    vec3f(1.60475, -0.10208, -0.00327),
    vec3f(-0.53108, 1.10813, -0.07276),
    vec3f(-0.07367, -0.00605, 1.07602),
);

fn rrt_and_odt_fit(v: vec3f) -> vec3f {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

fn aces_fitted(color: vec3f) -> vec3f {
    let fitted = ACES_OUTPUT * rrt_and_odt_fit(ACES_INPUT * color);
    return clamp(fitted, vec3f(0.0), vec3f(1.0));
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let texel = textureLoad(input_texture, vec2i(in.position.xy), 0);
    let exposed = max(texel.rgb, vec3f(0.0)) * tonemap.exposure_scale;
    // Only one preset exists; `tonemap.preset` selects among later curves.
    let mapped = aces_fitted(exposed);
    // Alpha is coverage, not color, and passes through untouched.
    return vec4f(mapped, texel.a);
}
