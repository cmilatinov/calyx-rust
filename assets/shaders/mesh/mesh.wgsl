//#include "shaders/common/inputs.wgsl"

const MAX_BONE_INFLUENCE = 4u;

struct Instance {
    bone_transform_index: i32,
    object_id: u32,
    padding: vec2u,
    transform: mat4x4f,
};

struct MeshInstances {
    num_bones: u32,
    instances: array<Instance>,
};

struct BoneTransform {
    transform: mat4x4f,
};

struct BoneStorage {
    bones_size: u32,
    bones: array<BoneTransform>,
};

@group(1) @binding(0)
var<storage, read> mesh: MeshInstances;

@group(1) @binding(1)
var<storage, read> bones: BoneStorage;

// `local`, in the mesh's bind pose, posed by the bones of `vertex` and `instance`: each bone's
// transform weighted by the vertex's weight for it. W is 1 for a position and 0 for a direction,
// such as a normal or tangent. An instance without a bone palette returns `local` unchanged, and
// a bone index that is negative or past the palette adds nothing.
fn skinned_local(vertex: VertexIn, instance: Instance, local: vec4f) -> vec4f {
    if instance.bone_transform_index < 0 {
        return local;
    }
    var skinned: vec4f;
    for (var i = 0u; i < MAX_BONE_INFLUENCE; i++) {
        let bone_index = vertex.bone_indices[i];
        if bone_index < 0 {
            continue;
        }
        let transform_index =
            u32(instance.bone_transform_index) * mesh.num_bones + u32(bone_index);
        if transform_index >= bones.bones_size {
            continue;
        }
        skinned += (bones.bones[transform_index].transform * local) * vertex.bone_weights[i];
    }
    return skinned;
}
