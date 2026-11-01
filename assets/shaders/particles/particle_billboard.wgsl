//#include "shaders/common/camera.wgsl"

// The camera-facing billboard every particle draws as, shared by the colour
// pass (`particles.wgsl`) and the depth pass (`particles_depth.wgsl`).

struct VertexIn {
    @location(0) local_position: vec2f,
    @location(1) uv: vec2f,
    @location(2) position_size: vec4f,
    @location(3) color: vec4f,
};

struct VertexOut {
    @builtin(position) clip_position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

const MIN_PARTICLE_DIAMETER_PX: f32 = 6.0;

@group(1) @binding(0)
var particle_texture: texture_2d<f32>;

@group(1) @binding(1)
var particle_sampler: sampler;

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var output: VertexOut;
    // A particle sized zero draws nothing, rather than the minimum diameter:
    // the quad collapses to a point outside the clip volume.
    if input.position_size.w <= 0.0 {
        output.clip_position = vec4f(0.0, 0.0, -1.0, 1.0);
        output.uv = input.uv;
        output.color = vec4f(0.0);
        return output;
    }
    let clip_center = camera.projection * camera.view * vec4f(input.position_size.xyz, 1.0);
    let clip_w = max(abs(clip_center.w), 1e-5);
    let size_ndc = vec2f(
        abs(camera.projection[0][0]) * input.position_size.w / clip_w,
        abs(camera.projection[1][1]) * input.position_size.w / clip_w,
    );
    let min_size_ndc = vec2f(
        2.0 * MIN_PARTICLE_DIAMETER_PX / max(camera.viewport_size.x, 1.0),
        2.0 * MIN_PARTICLE_DIAMETER_PX / max(camera.viewport_size.y, 1.0),
    );
    let clamped_size_ndc = max(size_ndc, min_size_ndc);
    let clip_offset = vec4f(
        input.local_position.x * clamped_size_ndc.x * clip_w,
        input.local_position.y * clamped_size_ndc.y * clip_w,
        0.0,
        0.0,
    );

    output.clip_position = clip_center + clip_offset;
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

// Whether the fragment at `uv` lies outside the billboard's round disc.
fn outside_disc(uv: vec2f) -> bool {
    return length(uv * 2.0 - vec2f(1.0, 1.0)) > 1.0;
}
