struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) uv3: vec2<f32>,
    @location(6) model_0: vec4<f32>,
    @location(7) model_1: vec4<f32>,
    @location(8) model_2: vec4<f32>,
    @location(9) model_3: vec4<f32>,
    @location(10) color: vec4<f32>,
    @location(11) enable_normals: i32,
    @location(12) use_uv_colors: i32
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) enable_normals: i32
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
    near_plane: f32,
    far_plane: f32
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(
        vertex.model_0,
        vertex.model_1,
        vertex.model_2,
        vertex.model_3
    );
    var out: VertexOut;
    out.position = camera.projection * camera.view * model * vec4<f32>(vertex.position, 1.0);
    out.world_position = (model * vec4<f32>(vertex.position, 1.0)).xyz;
    out.world_normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    if (vertex.use_uv_colors > 0) {
        out.color = vec4<f32>(vertex.uv0, vertex.uv1);
    } else {
        out.color = vertex.color;
    }
    out.enable_normals = vertex.enable_normals;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let view_pos = vec3<f32>(
        camera.inverse_view[3][0],
        camera.inverse_view[3][1],
        camera.inverse_view[3][2]
    );
    let to_camera = normalize(view_pos - in.world_position);
    let normal = normalize(in.world_normal);
    var color = in.color;
    if (in.enable_normals > 0 && dot(normal, to_camera) < 0.0) {
        color.a *= 0.2;
    }
    return color;
}