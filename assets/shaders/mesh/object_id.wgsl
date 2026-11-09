//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"

// The object id of a static batch's one instance: its vertices carry each
// merged object's own id in their fourth UV channel instead.
const BATCHED_OBJECT_ID = 0xffffffffu;

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) @interpolate(flat) object_id: u32,
};

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: VertexOut;
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    let world_position = instance.transform * local_position;
    out.position = camera.projection * camera.view * world_position;
    out.object_id = select(
        instance.object_id,
        u32(vertex.uv3.x + 0.5),
        instance.object_id == BATCHED_OBJECT_ID,
    );
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) u32 {
    return in.object_id;
}
