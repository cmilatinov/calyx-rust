// Shared by the SSAO occlusion and blur shaders, and by the deferred lighting pass, which
// upsamples the occlusion with `ssao_visibility`.

// The most samples `SsaoUniform::kernel` holds.
const SSAO_MAX_SAMPLES: u32 = 64u;

// Depth at or beyond which a pixel holds no surface: the G-buffer's depth is
// cleared to 1.0 where the geometry pass drew nothing, and the skybox, drawn
// later, writes 0.9999999. Such a pixel is never occluded.
const SSAO_SKY_DEPTH: f32 = 0.9999999;

// View depth the occlusion target stores for sky pixels, so no surface
// shares a blur or upsample weight with them. Fits in a half float.
const SSAO_SKY_VIEW_Z: f32 = 60000.0;

struct SsaoUniform {
    // The camera's clip planes and the tangents of half its fields of view.
    // View space is left-handed: the camera looks down +z.
    near: f32,
    far: f32,
    tan_half_fov: vec2f,
    // Hemisphere radius, in view-space units.
    radius: f32,
    // Depth difference below which a sample does not count as occluded.
    bias: f32,
    // Exponent applied to the unoccluded fraction.
    intensity: f32,
    // How many kernel entries are used, 1..=SSAO_MAX_SAMPLES.
    sample_count: u32,
    // Hemisphere offsets around +z, in units of `radius`.
    kernel: array<vec4f, 64>,
};

fn ssao_is_sky(depth: f32) -> bool {
    return depth >= SSAO_SKY_DEPTH;
}

// View distance of a depth-buffer value, as `CameraPlanes::linear_depth`:
// the projection is `perspective_lh` with a [-1, 1] depth range, of which the
// depth buffer keeps [0, 1].
fn ssao_linear_depth(near: f32, far: f32, depth: f32) -> f32 {
    return 2.0 * near * far / (far + near - depth * (far - near));
}

// The view-space position at `uv` ((0, 0) top left) and view distance `z`.
fn ssao_view_position(tan_half_fov: vec2f, uv: vec2f, z: f32) -> vec3f {
    let ndc = vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    return vec3f(ndc * tan_half_fov * z, z);
}

// The uv ((0, 0) top left) a view-space position projects to.
fn ssao_project(tan_half_fov: vec2f, position: vec3f) -> vec2f {
    let ndc = position.xy / (tan_half_fov * position.z);
    return vec2f(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// How much two view depths belong to the same surface, from 1 (equal) down
// to 0 at a 10% difference.
fn ssao_depth_weight(z: f32, reference: f32) -> f32 {
    return max(0.0, 1.0 - abs(z - reference) / (0.1 * abs(reference) + 1e-4));
}

// The ambient visibility SSAO measured for the full-resolution pixel at `position`
// (framebuffer coordinates), whose view depth is `z`, from the half-resolution `occlusion`
// target: visibility in red, view depth in green. It is upsampled from the four nearest
// texels, weighted bilinearly and by how close their view depth is to `z`, so it does not
// bleed across silhouettes. A pixel whose four texels all lie on other surfaces, such as a
// thin object between them, keeps a visibility of 1 rather than borrow another surface's.
fn ssao_visibility(occlusion: texture_2d<f32>, position: vec2f, z: f32) -> f32 {
    let half_size = vec2i(textureDimensions(occlusion));
    // Occlusion texel (i, j) was computed at full-resolution pixel (2i, 2j).
    let coord = position * 0.5 - 0.25;
    let base = vec2i(floor(coord));
    let f = coord - floor(coord);
    var sum = 0.0;
    var weights = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let at = clamp(base + vec2i(x, y), vec2i(0), half_size - 1);
            let texel = textureLoad(occlusion, at, 0);
            let bilinear = mix(1.0 - f.x, f.x, f32(x)) * mix(1.0 - f.y, f.y, f32(y));
            let weight = bilinear * ssao_depth_weight(texel.g, z);
            sum += texel.r * weight;
            weights += weight;
        }
    }
    if weights > 0.0 {
        return sum / weights;
    }
    return 1.0;
}
