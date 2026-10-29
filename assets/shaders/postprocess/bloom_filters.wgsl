// Filters shared by the bloom passes. Each samples `source` with bilinear
// filtering around `uv`, in steps of one texel of `source`.

// The 13-tap downsample from Jimenez, "Next Generation Post Processing in
// Call of Duty: Advanced Warfare" (SIGGRAPH 2014): five overlapping 2x2 box
// filters, weighted so the result is a low-pass average of a 4x4 region. The
// weights sum to one, so a flat image keeps its value.
fn bloom_downsample(source: texture_2d<f32>, source_sampler: sampler, uv: vec2f) -> vec3f {
    let texel = 1.0 / vec2f(textureDimensions(source));
    let a = textureSampleLevel(source, source_sampler, uv + texel * vec2f(-2.0, -2.0), 0.0).rgb;
    let b = textureSampleLevel(source, source_sampler, uv + texel * vec2f(0.0, -2.0), 0.0).rgb;
    let c = textureSampleLevel(source, source_sampler, uv + texel * vec2f(2.0, -2.0), 0.0).rgb;
    let d = textureSampleLevel(source, source_sampler, uv + texel * vec2f(-2.0, 0.0), 0.0).rgb;
    let e = textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
    let f = textureSampleLevel(source, source_sampler, uv + texel * vec2f(2.0, 0.0), 0.0).rgb;
    let g = textureSampleLevel(source, source_sampler, uv + texel * vec2f(-2.0, 2.0), 0.0).rgb;
    let h = textureSampleLevel(source, source_sampler, uv + texel * vec2f(0.0, 2.0), 0.0).rgb;
    let i = textureSampleLevel(source, source_sampler, uv + texel * vec2f(2.0, 2.0), 0.0).rgb;
    let j = textureSampleLevel(source, source_sampler, uv + texel * vec2f(-1.0, -1.0), 0.0).rgb;
    let k = textureSampleLevel(source, source_sampler, uv + texel * vec2f(1.0, -1.0), 0.0).rgb;
    let l = textureSampleLevel(source, source_sampler, uv + texel * vec2f(-1.0, 1.0), 0.0).rgb;
    let m = textureSampleLevel(source, source_sampler, uv + texel * vec2f(1.0, 1.0), 0.0).rgb;
    return e * 0.125
        + (a + c + g + i) * 0.03125
        + (b + d + f + h) * 0.0625
        + (j + k + l + m) * 0.125;
}

// A 3x3 tent filter, weights 1-2-1 by 1-2-1 over 16, for upsampling a
// smaller mip without blocky edges. The weights sum to one.
fn bloom_upsample(source: texture_2d<f32>, source_sampler: sampler, uv: vec2f) -> vec3f {
    let texel = 1.0 / vec2f(textureDimensions(source));
    var sum = textureSampleLevel(source, source_sampler, uv, 0.0).rgb * 4.0;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(-1.0, 0.0), 0.0).rgb * 2.0;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(1.0, 0.0), 0.0).rgb * 2.0;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(0.0, -1.0), 0.0).rgb * 2.0;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(0.0, 1.0), 0.0).rgb * 2.0;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(-1.0, -1.0), 0.0).rgb;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(1.0, -1.0), 0.0).rgb;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(-1.0, 1.0), 0.0).rgb;
    sum += textureSampleLevel(source, source_sampler, uv + texel * vec2f(1.0, 1.0), 0.0).rgb;
    return sum / 16.0;
}
