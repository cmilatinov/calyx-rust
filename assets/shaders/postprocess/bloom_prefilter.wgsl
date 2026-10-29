//#include "shaders/fullscreen_pass.wgsl"
//#include "shaders/postprocess/bloom_filters.wgsl"

// The first bloom pass: downsamples the HDR scene to the first mip at half
// size and keeps only the light above the threshold, with a soft knee.

struct BloomUniform {
    // Brightness where bloom reaches full strength.
    threshold: f32,
    // Half the width of the soft knee around the threshold, in brightness.
    knee: f32,
    // The composite's scale of the blurred light.
    intensity: f32,
    _padding: f32,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;

@group(0) @binding(2)
var<uniform> bloom: BloomUniform;

// Brightest channel, so saturated lights bloom in their own color.
fn brightness(color: vec3f) -> f32 {
    return max(color.r, max(color.g, color.b));
}

// Scales `color` by how much of it lies above the threshold. Below
// `threshold - knee` nothing passes; above `threshold + knee` everything above
// the threshold passes; between, a quadratic curve joins the two.
fn soft_threshold(color: vec3f) -> vec3f {
    let value = brightness(color);
    let knee = max(bloom.knee, 1e-5);
    let soft = clamp(value - bloom.threshold + knee, 0.0, 2.0 * knee);
    let curve = soft * soft / (4.0 * knee);
    let contribution = max(curve, value - bloom.threshold) / max(value, 1e-5);
    return color * contribution;
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    // Negative or overflowed scene color must not spread through the blur.
    let color = clamp(bloom_downsample(input_texture, input_sampler, in.uv), vec3f(0.0), vec3f(65000.0));
    return vec4f(soft_threshold(color), 1.0);
}
