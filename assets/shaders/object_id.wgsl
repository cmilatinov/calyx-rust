//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"
//#include "shaders/mesh.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) object_id: u32,
};

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: VertexOut;
    let world_position = instance.transform * vec4f(vertex.position, 1.0);
    out.position = camera.projection * camera.view * world_position;
    out.object_id = instance.object_id;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) u32 {
    return in.object_id;
}
