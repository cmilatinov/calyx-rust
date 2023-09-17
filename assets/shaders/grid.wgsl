struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(1) near_point: vec3<f32>,
    @location(2) far_point: vec3<f32>,
};

struct FragmentOut {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

struct CameraUniforms {
    projection: mat4x4<f32>,
    view: mat4x4<f32>,
    inverse_projection: mat4x4<f32>,
    inverse_view: mat4x4<f32>,
    near_plane: f32,
    far_plane: f32
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

fn to_world_space(clip_coords: vec3<f32>) -> vec3<f32> {
    let pos = camera.inverse_view * camera.inverse_projection * vec4<f32>(clip_coords, 1.0);
    return pos.xyz / pos.w;
}

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let pos = vertex.position.xy;
    var out: VertexOut;
    out.position = vec4<f32>(vertex.position, 1.0);
    out.near_point = to_world_space(vec3<f32>(pos, 0.0));
    out.far_point = to_world_space(vec3<f32>(pos, 1.0));
    return out;
}

fn compute_depth(frag_pos: vec3<f32>) -> f32 {
    let clip_space: vec4<f32> = camera.projection * camera.view * vec4<f32>(frag_pos, 1.0);
    return clip_space.z / clip_space.w;
}

fn linearize_depth(depth: f32) -> f32 {
    return (2.0 * camera.near_plane * camera.far_plane) /
        (camera.far_plane + camera.near_plane - depth * (camera.far_plane - camera.near_plane)) /
        camera.far_plane;
}

fn grid(frag_pos: vec3<f32>, grid_color: vec3<f32>, line_width: f32, scale: f32) -> vec4<f32> {
    let coord = frag_pos.xz * scale;
    let derivative = fwidth(coord);
    let grid = max(abs(fract(coord - vec2<f32>(0.5)) - vec2<f32>(0.5)) - vec2<f32>(line_width), vec2<f32>(0.0)) / derivative;
    var color = vec4<f32>(grid_color, 1.0 - min(min(grid.x, grid.y), 1.0));
    let min_x = min(derivative.x, 0.05);
    let min_z = min(derivative.y, 0.05);
    if (frag_pos.x > -min_x && frag_pos.x < min_x) {
        color.x = 0.1;
        color.y = 0.1;
        color.z = 1.0;
    } else if (frag_pos.z > -min_z && frag_pos.z < min_z) {
        color.x = 1.0;
        color.y = 0.1;
        color.z = 0.1;
    }
    return color;
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    let t = -in.near_point.y / (in.far_point.y - in.near_point.y);
    if (t < 0.0) {
        discard;
    }

    var out: FragmentOut;
    let frag_pos = in.near_point + t * (in.far_point - in.near_point);
    out.color =
        grid(frag_pos, vec3<f32>(0.05), 0.0001, 1.0) +
        grid(frag_pos, vec3<f32>(0.025), 0.001, 0.1);

    let view_pos = vec3<f32>(
        camera.inverse_view[3][0],
        camera.inverse_view[3][1],
        camera.inverse_view[3][2]
    );
    let depth = compute_depth(frag_pos);
    let dist = distance(view_pos.xz, frag_pos.xz);
    let alpha = pow(clamp(dist / 300.0, 0.0, 1.0) - 1.0, 2.0);
    out.color.a *= alpha;

    if (out.color.a > 0.0) {
        out.depth = depth;
    } else {
        out.depth = 0.999;
    }

    return out;
}