//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"

// The depth-only shadow pass: every shadow-casting mesh of the scene pass's draw list, from one
// cascade or spot light at a time. It reads the same vertex inputs, instance buffer and bone
// palette as `shaders/mesh/pbr.wgsl`, so a skinned mesh casts its posed shadow. Group 0 is the
// camera of `shaders/common/camera.wgsl`, here the cascade's or spot light's view.

@vertex
fn vs_main(vertex: VertexIn) -> @builtin(position) vec4f {
    let instance = mesh.instances[vertex.instance];
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    return camera.projection * camera.view * (instance.transform * local_position);
}

// Depth only: no color target.
@fragment
fn fs_main() {}
