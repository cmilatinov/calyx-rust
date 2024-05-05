const MAX_INSTANCES = 30u;
const MAX_BONE_INFLUENCE = 4u;

struct Instance {
    bone_transform_index: i32,
    transform: mat4x4f,
};

struct MeshUniforms {
    num_bones: u32,
    instances: array<Instance, MAX_INSTANCES>
};

struct BoneTransform {
    transform: mat4x4f,
};

struct BoneStorage {
    bones_size: u32,
    bones: array<BoneTransform>,
};

@group(1) @binding(0)
var<uniform> mesh: MeshUniforms;

@group(1) @binding(1)
var<storage, read> bones: BoneStorage;
