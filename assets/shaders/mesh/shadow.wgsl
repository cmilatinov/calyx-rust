//#include "shaders/common/inputs.wgsl"
//#include "shaders/mesh/mesh.wgsl"

// The depth-only shadow pass: every shadow-casting mesh of the scene pass's draw list, from one
// cascade or spot light at a time. It reads the same vertex inputs, instance buffer and bone
// palette as `shaders/mesh/pbr.wgsl`, so a skinned mesh casts its posed shadow.

struct ShadowView {
    // World to the light's clip space.
    view_projection: mat4x4f,
};

@group(0) @binding(0)
var<uniform> shadow_view: ShadowView;

@vertex
fn vs_main(vertex: VertexIn) -> @builtin(position) vec4f {
    let instance = mesh.instances[vertex.instance];
    var local_position = vec4f(vertex.position, 1.0);
    if instance.bone_transform_index >= 0 {
        var skinned_position: vec4f;
        for (var i = 0u; i < MAX_BONE_INFLUENCE; i++) {
            let bone_index = vertex.bone_indices[i];
            let bone_weight = vertex.bone_weights[i];
            if (bone_index < 0) {
                continue;
            }
            let transform_index =
                u32(instance.bone_transform_index) * mesh.num_bones + u32(bone_index);
            if (transform_index >= bones.bones_size) {
                continue;
            }
            skinned_position +=
                (bones.bones[transform_index].transform * local_position) * bone_weight;
        }
        local_position = skinned_position;
    }
    return shadow_view.view_projection * (instance.transform * local_position);
}

// Depth only: no color target.
@fragment
fn fs_main() {}
