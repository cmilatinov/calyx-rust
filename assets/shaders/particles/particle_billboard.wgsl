//#include "shaders/common/camera.wgsl"

// The camera-facing billboard every particle draws as, shared by the colour
// pass (`particles.wgsl`) and the depth pass (`particles_depth.wgsl`).

struct VertexIn {
    @location(0) local_position: vec2f,
    @location(1) uv: vec2f,
    @location(2) position_size: vec4f,
    @location(3) color: vec4f,
    @location(4) direction_min_size: vec4f,
};

struct VertexOut {
    @builtin(position) clip_position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

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
        2.0 * input.direction_min_size.w / max(camera.viewport_size.x, 1.0),
        2.0 * input.direction_min_size.w / max(camera.viewport_size.y, 1.0),
    );
    let clamped_size_ndc = max(size_ndc, min_size_ndc);
    var clip_offset = vec4f(
        input.local_position.x * clamped_size_ndc.x * clip_w,
        input.local_position.y * clamped_size_ndc.y * clip_w,
        0.0,
        0.0,
    );

    let length_scale = length(input.direction_min_size.xyz);
    if length_scale > 0.0 {
        // Work in physical pixels so rotation preserves width/aspect on a
        // non-square viewport. The shared vertex also shapes both depth passes.
        let clip_direction = camera.projection * camera.view * vec4f(input.direction_min_size.xyz, 0.0);
        let projected = (clip_direction.xy * clip_center.w - clip_center.xy * clip_direction.w)
            * camera.viewport_size;
        var axis = vec2f(0.0, 1.0);
        if dot(projected, projected) > 1e-10 {
            axis = normalize(projected);
        }
        let across = vec2f(axis.y, -axis.x);
        let width_px = max(size_ndc.y * camera.viewport_size.y * 0.5, input.direction_min_size.w);
        let offset_px = width_px * (
            across * input.local_position.x + axis * input.local_position.y * length_scale
        );
        clip_offset = vec4f(2.0 * offset_px / max(camera.viewport_size, vec2f(1.0)) * clip_w, 0.0, 0.0);
    }

    output.clip_position = clip_center + clip_offset;
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

// Whether the fragment at `uv` lies outside the billboard's round disc.
fn outside_disc(uv: vec2f) -> bool {
    return length(uv * 2.0 - vec2f(1.0, 1.0)) > 1.0;
}
