//#include "shaders/common/fullscreen_pass.wgsl"

// FXAA 3.11 Quality, preset 12, ported from Timothy Lottes' reference
// (NVIDIA, public domain): `FxaaPixelShader`'s PC quality path.
//
// It runs after tone mapping on display-linear color. Edge detection needs a
// perceptual luma, so luma is the square root of linear Rec. 601 luma, close
// to the gamma-encoded luma the reference expects. Alpha is read from the
// pixel itself and never filtered.

struct FxaaUniform {
    // Minimum local contrast, relative to the brightest neighbour, that counts
    // as an edge. Lower finds more edges.
    edge_threshold: f32,
    // How much sub-pixel aliasing is removed, 0 (none) to 1 (softest).
    subpixel: f32,
    _padding: vec2f,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;

@group(0) @binding(2)
var<uniform> fxaa: FxaaUniform;

// Contrast below this is never an edge, so dark noise is left alone.
const EDGE_THRESHOLD_MIN: f32 = 0.0833;

// Preset 12's search steps along the edge, in pixels.
const SEARCH_STEPS: u32 = 5u;
const SEARCH_STEP_SIZES = array<f32, 5>(1.0, 1.5, 2.0, 4.0, 12.0);

fn luma(color: vec3f) -> f32 {
    return sqrt(dot(clamp(color, vec3f(0.0), vec3f(1.0)), vec3f(0.299, 0.587, 0.114)));
}

fn sample_color(uv: vec2f) -> vec3f {
    return textureSampleLevel(input_texture, input_sampler, uv, 0.0).rgb;
}

fn sample_luma(uv: vec2f) -> f32 {
    return luma(sample_color(uv));
}

fn sample_luma_offset(uv: vec2f, offset: vec2f, texel: vec2f) -> f32 {
    return luma(sample_color(uv + offset * texel));
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let center = textureLoad(input_texture, pixel, 0);
    let texel = 1.0 / vec2f(textureDimensions(input_texture));
    let pos_m = in.position.xy * texel;

    let luma_m = luma(center.rgb);
    var luma_s = sample_luma_offset(pos_m, vec2f(0.0, 1.0), texel);
    let luma_e = sample_luma_offset(pos_m, vec2f(1.0, 0.0), texel);
    var luma_n = sample_luma_offset(pos_m, vec2f(0.0, -1.0), texel);
    let luma_w = sample_luma_offset(pos_m, vec2f(-1.0, 0.0), texel);

    let range_max = max(max(luma_n, luma_w), max(luma_e, max(luma_s, luma_m)));
    let range_min = min(min(luma_n, luma_w), min(luma_e, min(luma_s, luma_m)));
    let range = range_max - range_min;
    if range < max(EDGE_THRESHOLD_MIN, range_max * fxaa.edge_threshold) {
        return center;
    }

    let luma_nw = sample_luma_offset(pos_m, vec2f(-1.0, -1.0), texel);
    let luma_se = sample_luma_offset(pos_m, vec2f(1.0, 1.0), texel);
    let luma_ne = sample_luma_offset(pos_m, vec2f(1.0, -1.0), texel);
    let luma_sw = sample_luma_offset(pos_m, vec2f(-1.0, 1.0), texel);

    // Is the edge horizontal or vertical?
    let luma_ns = luma_n + luma_s;
    let luma_we = luma_w + luma_e;
    let luma_nese = luma_ne + luma_se;
    let luma_nwne = luma_nw + luma_ne;
    let luma_nwsw = luma_nw + luma_sw;
    let luma_swse = luma_sw + luma_se;
    let edge_horizontal = abs(-2.0 * luma_w + luma_nwsw)
        + abs(-2.0 * luma_m + luma_ns) * 2.0
        + abs(-2.0 * luma_e + luma_nese);
    let edge_vertical = abs(-2.0 * luma_s + luma_swse)
        + abs(-2.0 * luma_m + luma_we) * 2.0
        + abs(-2.0 * luma_n + luma_nwne);
    let horizontal_span = edge_horizontal >= edge_vertical;

    // Sub-pixel blend amount from the 3x3 neighbourhood's low-pass.
    let subpixel_a = (luma_ns + luma_we) * 2.0 + luma_nwsw + luma_nese;
    let subpixel_b = subpixel_a * (1.0 / 12.0) - luma_m;
    let subpixel_c = saturate(abs(subpixel_b) / range);
    let subpixel_f = (-2.0 * subpixel_c + 3.0) * subpixel_c * subpixel_c;
    let subpixel_h = subpixel_f * subpixel_f * fxaa.subpixel;

    // Pick the side of the pixel the edge lies on.
    if !horizontal_span {
        luma_n = luma_w;
        luma_s = luma_e;
    }
    var length_sign = select(texel.x, texel.y, horizontal_span);
    let gradient_n = luma_n - luma_m;
    let gradient_s = luma_s - luma_m;
    let pair_n = abs(gradient_n) >= abs(gradient_s);
    let gradient_scaled = max(abs(gradient_n), abs(gradient_s)) * 0.25;
    if pair_n {
        length_sign = -length_sign;
    }
    let luma_nn = select(luma_s, luma_n, pair_n) + luma_m;
    let luma_edge = luma_nn * 0.5;
    let luma_m_below = luma_m - luma_edge < 0.0;

    // Walk along the edge in both directions until its contrast ends.
    var pos_b = pos_m;
    var search_step = vec2f(0.0, texel.y);
    if horizontal_span {
        pos_b.y += length_sign * 0.5;
        search_step = vec2f(texel.x, 0.0);
    } else {
        pos_b.x += length_sign * 0.5;
    }
    var pos_n = pos_b - search_step * SEARCH_STEP_SIZES[0];
    var pos_p = pos_b + search_step * SEARCH_STEP_SIZES[0];
    var luma_end_n = sample_luma(pos_n) - luma_edge;
    var luma_end_p = sample_luma(pos_p) - luma_edge;
    var done_n = abs(luma_end_n) >= gradient_scaled;
    var done_p = abs(luma_end_p) >= gradient_scaled;
    // As in the reference, each side steps once more after its last sample.
    if !done_n {
        pos_n -= search_step * SEARCH_STEP_SIZES[1];
    }
    if !done_p {
        pos_p += search_step * SEARCH_STEP_SIZES[1];
    }
    for (var i = 2u; i < SEARCH_STEPS; i++) {
        if done_n && done_p {
            break;
        }
        if !done_n {
            luma_end_n = sample_luma(pos_n) - luma_edge;
            done_n = abs(luma_end_n) >= gradient_scaled;
        }
        if !done_p {
            luma_end_p = sample_luma(pos_p) - luma_edge;
            done_p = abs(luma_end_p) >= gradient_scaled;
        }
        if !done_n {
            pos_n -= search_step * SEARCH_STEP_SIZES[i];
        }
        if !done_p {
            pos_p += search_step * SEARCH_STEP_SIZES[i];
        }
    }

    // Offset the sample towards the nearer end of the edge.
    var distance_n = pos_m.y - pos_n.y;
    var distance_p = pos_p.y - pos_m.y;
    if horizontal_span {
        distance_n = pos_m.x - pos_n.x;
        distance_p = pos_p.x - pos_m.x;
    }
    let toward_n = distance_n < distance_p;
    let nearest = min(distance_n, distance_p);
    let good_span = select(
        (luma_end_p < 0.0) != luma_m_below,
        (luma_end_n < 0.0) != luma_m_below,
        toward_n,
    );
    let pixel_offset = select(0.0, 0.5 - nearest / (distance_n + distance_p), good_span);
    let offset = max(pixel_offset, subpixel_h);

    var pos_f = pos_m;
    if horizontal_span {
        pos_f.y += offset * length_sign;
    } else {
        pos_f.x += offset * length_sign;
    }
    return vec4f(sample_color(pos_f), center.a);
}
