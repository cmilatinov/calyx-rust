//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) world_normal: vec3f,
    @location(2) color: vec4f,
    @location(3) enable_normals: i32
};

const MAX_INSTANCES = 30;

struct Instance {
    transform: mat4x4f,
    color: vec4f,
    enable_normals: i32,
    use_uv_colors: i32,
};

@group(1) @binding(0)
var<uniform> instances: array<Instance, MAX_INSTANCES>;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = instances[vertex.instance];
    var out: VertexOut;
    out.position = camera.projection * camera.view * instance.transform * vec4f(vertex.position, 1.0);
    out.world_position = (instance.transform * vec4f(vertex.position, 1.0)).xyz;
    out.world_normal = (instance.transform * vec4f(vertex.normal, 0.0)).xyz;
    if (instance.use_uv_colors > 0) {
        out.color = vec4f(vertex.uv0, vertex.uv1);
    } else {
        out.color = instance.color;
    }
    out.enable_normals = instance.enable_normals;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let view_pos = vec3f(
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