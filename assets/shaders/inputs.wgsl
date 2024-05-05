struct VertexIn {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv0: vec2f,
    @location(3) uv1: vec2f,
    @location(4) uv2: vec2f,
    @location(5) uv3: vec2f,
    @location(6) bone_indices: vec4i,
    @location(7) bone_weights: vec4f
};
