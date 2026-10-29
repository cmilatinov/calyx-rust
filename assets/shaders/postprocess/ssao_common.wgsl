// Shared by the SSAO occlusion, blur and composite shaders.

// The most samples `SsaoUniform::kernel` holds.
const SSAO_MAX_SAMPLES: u32 = 64u;

// Depth at or beyond which a pixel is sky: the skybox writes 0.9999999 and
// the depth buffer is cleared to 1.0. Sky is never occluded.
const SSAO_SKY_DEPTH: f32 = 0.9999999;

// View depth the occlusion target stores for sky pixels, so no surface
// shares a blur or upsample weight with them. Fits in a half float.
const SSAO_SKY_VIEW_Z: f32 = 60000.0;

struct SsaoUniform {
    // The camera projection the scene's depth was written through, and its
    // inverse. View space is left-handed: the camera looks down +z.
    projection: mat4x4f,
    inverse_projection: mat4x4f,
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

// The view-space position at `uv` ((0, 0) top left) with depth `depth`.
fn ssao_view_position(inverse_projection: mat4x4f, uv: vec2f, depth: f32) -> vec3f {
    let ndc = vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let position = inverse_projection * vec4f(ndc, depth, 1.0);
    return position.xyz / position.w;
}

// How much two view depths belong to the same surface, from 1 (equal) down
// to 0 at a 10% difference.
fn ssao_depth_weight(z: f32, reference: f32) -> f32 {
    return max(0.0, 1.0 - abs(z - reference) / (0.1 * abs(reference) + 1e-4));
}
