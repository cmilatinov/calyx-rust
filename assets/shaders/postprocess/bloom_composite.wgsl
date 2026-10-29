//#include "shaders/fullscreen_pass.wgsl"
//#include "shaders/postprocess/bloom_filters.wgsl"

// Adds the blurred light of the first bloom mip onto the HDR scene color.

struct BloomUniform {
    threshold: f32,
    knee: f32,
    // Scale of the added light, already divided by the mip count, since the
    // first mip holds the sum of every mip.
    intensity: f32,
    _padding: f32,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var bloom_texture: texture_2d<f32>;

@group(0) @binding(2)
var bloom_sampler: sampler;

@group(0) @binding(3)
var<uniform> bloom: BloomUniform;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let texel = textureLoad(input_texture, vec2i(in.position.xy), 0);
    let glow = bloom_upsample(bloom_texture, bloom_sampler, in.uv);
    // Alpha is coverage, not color, and passes through untouched.
    return vec4f(texel.rgb + glow * bloom.intensity, texel.a);
}
