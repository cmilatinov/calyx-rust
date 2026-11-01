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
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    return shadow_view.view_projection * (instance.transform * local_position);
}

// Depth only: no color target.
@fragment
fn fs_main() {}
