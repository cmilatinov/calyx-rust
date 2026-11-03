//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"
//#include "shaders/postprocess/ssao_common.wgsl"

// Screen-space ambient occlusion at half resolution, between the geometry pass and the deferred
// lighting pass. Each texel takes its view-space position from the G-buffer's depth and its
// normal from the G-buffer, then tests a hemisphere of kernel samples around the normal,
// rotated per texel by a 4x4 noise pattern, against the depth. Writes the unoccluded fraction,
// raised to the intensity, to red, and the view depth to green for the depth-aware blur and the
// lighting pass's upsample.

// The G-buffer's depth: at this point in the frame it holds the opaque PBR surfaces only.
@group(0) @binding(0)
var depth_texture: texture_depth_2d;

@group(0) @binding(1)
var<uniform> ssao: SsaoUniform;

// The G-buffer's normals: the view-space geometric normal, octahedral, in BA.
@group(0) @binding(2)
var normals_texture: texture_2d<f32>;

// Rotation of each pixel of a 4x4 tile, in sixteenths of a turn, in Bayer
// order so neighbouring pixels differ most. The 4x4 blur averages a full
// tile, which cancels the pattern.
const NOISE = array<f32, 16>(
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0,
);

fn full_size() -> vec2i {
    return vec2i(textureDimensions(depth_texture));
}

fn load_depth(pixel: vec2i) -> f32 {
    return textureLoad(depth_texture, clamp(pixel, vec2i(0), full_size() - 1), 0);
}

fn linear_depth(depth: f32) -> f32 {
    return ssao_linear_depth(ssao.near, ssao.far, depth);
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let half_pixel = vec2i(in.position.xy);
    let size = full_size();
    let pixel = min(half_pixel * 2, size - 1);
    let depth = load_depth(pixel);
    if ssao_is_sky(depth) {
        return vec4f(1.0, SSAO_SKY_VIEW_Z, 0.0, 1.0);
    }
    let uv = (vec2f(pixel) + 0.5) / vec2f(size);
    let center = ssao_view_position(ssao.tan_half_fov, uv, linear_depth(depth));

    // The geometric normal, not the shading one: the hemisphere is tested against the depth,
    // which holds the geometry, so a normal map would tip it into its own surface. It is
    // turned toward the camera for a back face drawn double-sided.
    var normal = gbuffer_decode_normal(textureLoad(normals_texture, pixel, 0).ba);
    if dot(normal, center) > 0.0 {
        normal = -normal;
    }

    // A tangent frame around the normal, rotated by the noise.
    let tile = half_pixel % vec2i(4);
    var noise = NOISE;
    let angle = noise[tile.y * 4 + tile.x] / 16.0 * 6.28318530718;
    let random = vec3f(cos(angle), sin(angle), 0.0);
    var tangent = random - normal * dot(random, normal);
    if length(tangent) < 1e-4 {
        tangent = cross(normal, vec3f(0.0, 0.0, 1.0));
    }
    tangent = normalize(tangent);
    let tbn = mat3x3f(tangent, cross(normal, tangent), normal);

    let count = clamp(ssao.sample_count, 1u, SSAO_MAX_SAMPLES);
    var occlusion = 0.0;
    for (var i = 0u; i < count; i++) {
        let sample_position = center + tbn * ssao.kernel[i].xyz * ssao.radius;
        if sample_position.z <= 0.0 {
            continue;
        }
        let sample_uv = ssao_project(ssao.tan_half_fov, sample_position);
        // Samples off screen have nothing to test against.
        if any(sample_uv < vec2f(0.0)) || any(sample_uv >= vec2f(1.0)) {
            continue;
        }
        let scene_depth = load_depth(vec2i(sample_uv * vec2f(size)));
        if ssao_is_sky(scene_depth) {
            continue;
        }
        let scene_z = linear_depth(scene_depth);
        // A surface in front of the sample hides it. Surfaces far in front of
        // this pixel, beyond the radius, fade out instead of darkening it.
        let range = smoothstep(0.0, 1.0, ssao.radius / max(abs(center.z - scene_z), 1e-5));
        if scene_z <= sample_position.z - ssao.bias {
            occlusion += range;
        }
    }
    let visibility = pow(1.0 - occlusion / f32(count), ssao.intensity);
    return vec4f(visibility, center.z, 0.0, 1.0);
}
