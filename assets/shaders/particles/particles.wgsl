//#include "shaders/particles/particle_billboard.wgsl"

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4f {
    let sampled = textureSampleLevel(particle_texture, particle_sampler, input.uv, 0.0);
    if outside_disc(input.uv) || sampled.a <= 0.001 {
        discard;
    }
    return vec4f(sampled.rgb * input.color.rgb, sampled.a * input.color.a);
}
