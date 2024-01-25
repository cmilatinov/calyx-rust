struct VertexIn {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(3) uv1: vec2<f32>,
    @location(4) uv2: vec2<f32>,
    @location(5) uv3: vec2<f32>,
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

const MAX_INSTANCES = 30;

struct Instance {
    transform: mat4x4<f32>,
    color: vec4<f32>,
    enable_normals: i32,
    use_uv_colors: i32,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@group(1) @binding(0)
var<uniform> instances: array<Instance, MAX_INSTANCES>;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = instances[vertex.instance];
    var out: VertexOut;
    out.position = camera.projection * camera.view * instance.transform * vec4<f32>(vertex.position, 1.0);
    out.world_position = (instance.transform * vec4<f32>(vertex.position, 1.0)).xyz;
    out.world_normal = (instance.transform * vec4<f32>(vertex.normal, 0.0)).xyz;
    if (instance.use_uv_colors > 0) {
        out.color = vec4<f32>(vertex.uv0, vertex.uv1);
    } else {
        out.color = instance.color;
    }
    out.enable_normals = instance.enable_normals;
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