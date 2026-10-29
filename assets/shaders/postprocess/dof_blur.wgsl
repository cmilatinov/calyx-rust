//#include "shaders/fullscreen_pass.wgsl"
//#include "shaders/postprocess/dof_common.wgsl"

// Depth of field, second step: a scatter-as-gather disc blur over a golden
// angle spiral (after Dennis Gustafsson, "Bokeh depth of field in a single
// pass", 2018). A tap counts when its own circle of confusion reaches the
// pixel, so each pixel blurs by its neighbours' blur, not only its own:
//
// - Far field: a tap behind the pixel is clamped to twice the pixel's own
//   radius, so a sharp or near object never picks up the blurred background.
// - Near field: a tap in front of the pixel is not clamped, so a blurred
//   foreground object spreads over the sharp pixels behind it.
//
// The spiral starts one pixel out, and a tap's weight ramps from 0 to 1 as
// its radius goes from half a pixel under the tap's distance to half a pixel
// over it. A pixel whose radius is under half a pixel, with no blurred
// foreground over it, therefore keeps its exact input value.

const GOLDEN_ANGLE: f32 = 2.39996323;

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var coc_texture: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> dof: DofUniform;

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = 1.0 / vec2f(textureDimensions(input_texture));
    let uv = in.position.xy * texel;
    let center_coc = textureLoad(coc_texture, pixel, 0).r;
    let center_size = abs(center_coc);

    // The running average of the taps. Each tap moves it towards the tap's
    // color by its weight, so a tap of weight zero leaves it exactly as is.
    var average = textureLoad(input_texture, pixel, 0);
    var count = 1.0;
    var radius = 1.0;
    var angle = 0.0;
    loop {
        if radius >= dof.max_blur {
            break;
        }
        let tap_uv = uv + vec2f(cos(angle), sin(angle)) * texel * radius;
        let tap_color = textureSampleLevel(input_texture, linear_sampler, tap_uv, 0.0);
        let tap_coc = textureSampleLevel(coc_texture, linear_sampler, tap_uv, 0.0).r;
        var tap_size = abs(tap_coc);
        if tap_coc > center_coc {
            // Behind the pixel: it may not blur over it.
            tap_size = min(tap_size, center_size * 2.0);
        }
        let weight = smoothstep(radius - 0.5, radius + 0.5, tap_size);
        count += 1.0;
        average += (tap_color - average) * (weight / count);
        radius += dof.radius_scale / radius;
        angle += GOLDEN_ANGLE;
    }
    return average;
}
