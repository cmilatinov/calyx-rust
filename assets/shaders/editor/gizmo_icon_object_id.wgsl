//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) @interpolate(flat) object_id: u32,
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
    out.object_id = u32(vertex.uv3.x + 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) u32 {
    let sampled = textureSampleLevel(icon_texture, icon_sampler, in.uv, 0.0);
    if sampled.a <= 0.001 || in.object_id == 0u {
        discard;
    }
    return in.object_id;
}
