struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv0: vec2<f32>,
    @location(6) model_0: vec4<f32>,
    @location(7) model_1: vec4<f32>,
    @location(8) model_2: vec4<f32>,
    @location(9) model_3: vec4<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv0: vec2<f32>
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    near_plane: f32,
    far_plane: f32
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(vertex.position, 1.0);
    return out;
}

fn compute_depth(frag_pos: vec3<f32>) -> f32 {
    let clip_space: vec4<f32> = camera.view * vec4<f32>(frag_pos, 1.0);
    return ((1 / clip_space.z) - (1 / camera.near_plane)) /
        ((1 / camera.far_plane) - (1 / camera.near_plane));
}

fn linearize_depth(depth: f32) -> f32 {
    let z: f32 = depth * 2.0 - 1.0;
    return (2.0 * camera.near_plane * camera.far_plane) /
        (camera.far_plane + camera.near_plane - z * (camera.far_plane - camera.near_plane)) /
        camera.far_plane;
}

fn grid(frag)

//vec4 Grid(vec3 fragPos, vec3 gridColor, float lineWidth, float scale) {
//    vec2 coord = fragPos.xz * scale;
//    vec2 derivative = fwidth(coord);
//    vec2 grid = max(abs(fract(coord - 0.5) - 0.5) - lineWidth, 0.0) / derivative;
//    float line = min(grid.x, grid.y);
//    vec4 color = vec4(gridColor, 1.0 - min(line, 1.0));
//
//    float minX = min(derivative.x, 1.0);
//    float minZ = min(derivative.y, 1.0);
//    if (fragPos.x > -minX && fragPos.x < minX) {
//        color.xyz = vec3(0.1, 0.1, 1.0);
//    } else if (fragPos.z > -minZ && fragPos.z < minZ) {
//        color.xyz = vec3(1.0, 0.1, 0.1);
//    }
//
//    return color;
//}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
//    float t = -nearPoint.y / (farPoint.y - nearPoint.y);
//        if (t <= 0)
//        discard;
}