//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

@group(1) @binding(0)
var icon_texture: texture_2d<f32>;

@group(1) @binding(1)
var icon_sampler: sampler;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = camera.projection * camera.view * vec4f(vertex.position, 1.0);
    out.uv = vertex.uv0;
    out.color = vec4f(vertex.uv1, vertex.uv2);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let sampled = textureSampleLevel(icon_texture, icon_sampler, in.uv, 0.0);
    if sampled.a <= 0.001 {
        discard;
    }
    return vec4f(in.color.rgb, in.color.a * sampled.a);
}
