struct VertexOut {
    @builtin(position) position: vec4<f32>,
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    model: mat4x4<f32>
};

var<private> v_positions: array<vec4<f32>, 3> = array<vec4<f32>, 3>(
    vec4<f32>(1.0, 1.0, 10.0, 1.0),
    vec4<f32>(1.0, -1.0, 10.0, 1.0),
    vec4<f32>(-1.0, -1.0, 10.0, 1.0),
);

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOut {
    var out: VertexOut;
    out.position = camera.projection * camera.view * camera.model * v_positions[idx];
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}