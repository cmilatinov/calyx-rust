//#include "shaders/inputs.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@group(0) @binding(0)
var in_texture: texture_2d<f32>;

@group(0) @binding(1)
var in_sampler: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = vec4f(vertex.position, 1.0);
    out.uv = vertex.uv0;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    return textureSample(in_texture, in_sampler, in.uv);
}
