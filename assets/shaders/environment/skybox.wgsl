//#include "shaders/camera.wgsl"

struct VertexIn {
    @location(0) position: vec3f,
};

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uvw: vec3f,
};

struct FragmentOut {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4f,
};

@group(1) @binding(0)
var skybox_texture: texture_cube<f32>;

@group(1) @binding(1)
var skybox_sampler: sampler;

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let transform = mat4x4f(
        vec4f(1.0, 0.0, 0.0, 0.0),
        vec4f(0.0, 1.0, 0.0, 0.0),
        vec4f(0.0, 0.0, 1.0, 0.0),
        vec4f(camera.inverse_view[3].xyz, 1.0),
    );
    var out: VertexOut;
    out.position = camera.projection * camera.view * transform * vec4f(in.position, 1.0);
    out.uvw = in.position;
    return out;
}

fn tone_map_exposure(color: vec3f, exposure: f32) -> vec3f {
    let gamma = 0.9;
    var result = vec3f(1.0) - exp(-color * exposure);
    result = pow(result, vec3f(1.0 / gamma));
    return result;
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    var out: FragmentOut;
    let uvw = normalize(in.uvw);
    let color = textureSample(skybox_texture, skybox_sampler, uvw).rgb;
    out.color = vec4f(tone_map_exposure(color, 0.5), 1.0);
    out.depth = 0.9999999;
    return out;
}