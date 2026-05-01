//#include "shaders/camera.wgsl"

struct VsInput {
    @location(0) local_position: vec2f,
    @location(1) uv: vec2f,
    @location(2) position_size: vec4f,
    @location(3) color: vec4f,
};

struct VsOutput {
    @builtin(position) clip_position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

@group(1) @binding(0)
var particle_texture: texture_2d<f32>;

@group(1) @binding(1)
var particle_sampler: sampler;

@vertex
fn vs_main(input: VsInput) -> VsOutput {
    let right = normalize(camera.inverse_view[0].xyz);
    let up = normalize(camera.inverse_view[1].xyz);
    let world_center = input.position_size.xyz;
    let world_position = world_center
        + right * (input.local_position.x * input.position_size.w)
        + up * (input.local_position.y * input.position_size.w);

    var output: VsOutput;
    output.clip_position = camera.projection * camera.view * vec4f(world_position, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VsOutput) -> @location(0) vec4f {
    let sampled = textureSample(particle_texture, particle_sampler, input.uv);
    let centered_uv = input.uv * 2.0 - vec2f(1.0, 1.0);
    let radial_mask = clamp(1.0 - length(centered_uv), 0.0, 1.0);
    let alpha = sampled.a * input.color.a * radial_mask;
    if alpha <= 0.001 {
        discard;
    }
    return vec4f(sampled.rgb * input.color.rgb, alpha);
}
