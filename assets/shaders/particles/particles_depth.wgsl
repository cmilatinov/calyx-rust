//#include "shaders/particles/particle_billboard.wgsl"

// Writes the depth of the particle fragments that cover their pixel: inside
// the billboard's disc, with a coverage (the texture's alpha times the
// particle's) of at least `coverage`. It draws no colour, after every
// particle's colour, so particles still blend against each other as before.

struct ParticleDepthUniform {
    coverage: f32,
    _padding0: f32,
    _padding1: f32,
    _padding2: f32,
};

@group(2) @binding(0)
var<uniform> particle_depth: ParticleDepthUniform;

@fragment
fn fs_main(input: VertexOut) {
    let sampled = textureSampleLevel(particle_texture, particle_sampler, input.uv, 0.0);
    if outside_disc(input.uv) || sampled.a * input.color.a < particle_depth.coverage {
        discard;
    }
}
