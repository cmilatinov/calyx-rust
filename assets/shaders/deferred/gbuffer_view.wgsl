//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// The G-buffer debug view: shows one G-buffer target in the scene texture, in place of the lit
// image. The scene texture holds display-linear color, which the readback and egui sRGB-encode.
// Albedo and emissive are colors and show as such. Normals and the material are data, so they
// are written so that the displayed value is the stored one: a channel of 0.5 shows as 128.

struct GBufferViewUniform {
    // 0 albedo, 1 normals, 2 material, 3 emissive (`GBufferView`).
    view: u32,
    _padding0: u32,
    _padding1: u32,
    _padding2: u32,
};

@group(0) @binding(0)
var albedo_texture: texture_2d<f32>;

@group(0) @binding(1)
var normals_texture: texture_2d<f32>;

@group(0) @binding(2)
var material_texture: texture_2d<f32>;

@group(0) @binding(3)
var emissive_texture: texture_2d<f32>;

@group(0) @binding(4)
var<uniform> gbuffer_view: GBufferViewUniform;

fn srgb_to_linear(value: vec3f) -> vec3f {
    let low = value / 12.92;
    let high = pow((value + 0.055) / 1.055, vec3f(2.4));
    return select(high, low, value <= vec3f(0.04045));
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let texel = vec2i(in.position.xy);
    // Albedo alpha is 1 where the geometry pass drew a surface and 0 where it drew none.
    if textureLoad(albedo_texture, texel, 0).a == 0.0 {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    switch gbuffer_view.view {
        case 0u: {
            return vec4f(textureLoad(albedo_texture, texel, 0).rgb, 1.0);
        }
        case 1u: {
            // The view-space shading normal, each axis mapped from [-1, 1] to [0, 1].
            let normal = gbuffer_decode_normal(textureLoad(normals_texture, texel, 0).rg);
            return vec4f(srgb_to_linear(normal * 0.5 + 0.5), 1.0);
        }
        case 2u: {
            let material = textureLoad(material_texture, texel, 0).rgb;
            return vec4f(srgb_to_linear(material), 1.0);
        }
        default: {
            // Linear HDR, compressed into [0, 1] by Reinhard so bright emission keeps its hue.
            let emission = textureLoad(emissive_texture, texel, 0).rgb;
            return vec4f(emission / (1.0 + emission), 1.0);
        }
    }
}
