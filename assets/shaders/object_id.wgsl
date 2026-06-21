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
    var local_position = vec4f(vertex.position, 1.0);
    if instance.bone_transform_index >= 0 {
        var skinned_position: vec4f;
        for (var i = 0u; i < MAX_BONE_INFLUENCE; i++) {
            let bone_index = vertex.bone_indices[i];
            let bone_weight = vertex.bone_weights[i];
            if (bone_index < 0) {
                continue;
            }
            let transform_index = u32(instance.bone_transform_index) * mesh.num_bones + u32(bone_index);
            if (transform_index >= bones.bones_size) {
                continue;
            }
            skinned_position += (bones.bones[transform_index].transform * local_position) * bone_weight;
        }
        local_position = skinned_position;
    }
    let world_position = instance.transform * local_position;
    out.position = camera.projection * camera.view * world_position;
    out.object_id = instance.object_id;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) u32 {
    return in.object_id;
}
