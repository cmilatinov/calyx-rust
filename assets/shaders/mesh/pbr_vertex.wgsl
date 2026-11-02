//#include "shaders/common/inputs.wgsl"
//#include "shaders/common/camera.wgsl"
//#include "shaders/mesh/mesh.wgsl"

// The `vs_main` vertex entry point of the PBR mesh shaders: the forward pass,
// `shaders/mesh/pbr.wgsl`, and the G-buffer's geometry pass, `shaders/mesh/gbuffer.wgsl`. Both
// include this file, so a mesh, skinned or not, reaches both with the same world position,
// normal, tangent and clip position. The position is `@invariant`: the geometry pass tests
// against the depth the forward pass wrote, and the two pipelines must compute equal depths.

struct PbrVertexOut {
    @builtin(position) @invariant position: vec4f,
    @location(0) world_position: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    // World-space tangent along increasing U; w is the bitangent's handedness.
    @location(3) tangent: vec4f,
};

@vertex
fn vs_main(vertex: VertexIn) -> PbrVertexOut {
    let instance = mesh.instances[vertex.instance];
    var out: PbrVertexOut;
    let local_position = skinned_local(vertex, instance, vec4f(vertex.position, 1.0));
    let local_normal = skinned_local(vertex, instance, vec4f(vertex.normal, 0.0));
    let local_tangent = skinned_local(vertex, instance, vec4f(vertex.tangent.xyz, 0.0));
    out.world_position = (instance.transform * local_position).xyz;
    out.normal = (instance.transform * local_normal).xyz;
    out.tangent = vec4f((instance.transform * local_tangent).xyz, vertex.tangent.w);
    out.position =
        camera.projection *
        camera.view *
        vec4f(out.world_position, 1.0);
    out.uv = vertex.uv0;
    return out;
}
