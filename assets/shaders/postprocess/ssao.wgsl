//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/postprocess/ssao_common.wgsl"

// Screen-space ambient occlusion at half resolution. Each pixel rebuilds its
// view-space position and normal from depth, then tests a hemisphere of
// kernel samples around the normal, rotated per pixel by a 4x4 noise
// pattern, against the depth buffer. Writes the unoccluded fraction, raised
// to the intensity, to red, and the pixel's view depth to green for the
// depth-aware blur and upsample.

@group(0) @binding(0)
var depth_texture: texture_depth_2d;

@group(0) @binding(1)
var<uniform> ssao: SsaoUniform;

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

fn view_at(pixel: vec2i) -> vec3f {
    let uv = (vec2f(pixel) + 0.5) / vec2f(full_size());
    return ssao_view_position(ssao.tan_half_fov, uv, linear_depth(load_depth(pixel)));
}

// The difference to the neighbour on one axis that lies on the same surface:
// of the two in bounds, the one closer in depth. Picking the closer side
// keeps silhouettes from bending normals.
fn surface_delta(pixel: vec2i, center: vec3f, step: vec2i, limit: i32, along: i32) -> vec3f {
    let before = view_at(pixel - step);
    let after = view_at(pixel + step);
    let has_before = along > 0;
    let has_after = along + 1 < limit;
    let before_closer = abs(before.z - center.z) < abs(after.z - center.z);
    if has_before && (!has_after || before_closer) {
        return center - before;
    }
    return after - center;
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let half_pixel = vec2i(in.position.xy);
    let pixel = half_pixel * 2;
    let depth = load_depth(pixel);
    if ssao_is_sky(depth) {
        return vec4f(1.0, SSAO_SKY_VIEW_Z, 0.0, 1.0);
    }
    let size = full_size();
    let center = view_at(pixel);

    // The normal, rebuilt from depth and turned toward the camera.
    let dx = surface_delta(pixel, center, vec2i(1, 0), size.x, pixel.x);
    let dy = surface_delta(pixel, center, vec2i(0, 1), size.y, pixel.y);
    var normal = cross(dx, dy);
    if length(normal) < 1e-12 {
        normal = -center;
    }
    normal = normalize(normal);
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
        let uv = ssao_project(ssao.tan_half_fov, sample_position);
        // Samples off screen have nothing to test against.
        if any(uv < vec2f(0.0)) || any(uv >= vec2f(1.0)) {
            continue;
        }
        let scene_depth = load_depth(vec2i(uv * vec2f(size)));
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
