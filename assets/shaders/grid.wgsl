//#include "shaders/inputs.wgsl"
//#include "shaders/camera.wgsl"

struct VertexOut {
    @builtin(position) position: vec4f,
    @location(1) near_point: vec3f,
    @location(2) far_point: vec3f,
};

struct FragmentOut {
    @location(0) color: vec4f,
    @builtin(frag_depth) depth: f32,
};

fn to_world_space(clip_coords: vec3f) -> vec3f {
    let pos = camera.inverse_view * camera.inverse_projection * vec4f(clip_coords, 1.0);
    return pos.xyz / pos.w;
}

@vertex
fn vs_main(vertex: VertexIn) -> VertexOut {
    let pos = vertex.position.xy;
    var out: VertexOut;
    out.position = vec4f(vertex.position, 1.0);
    out.near_point = to_world_space(vec3f(pos, 0.0));
    out.far_point = to_world_space(vec3f(pos, 1.0));
    return out;
}

fn compute_depth(frag_pos: vec3f) -> f32 {
    let clip_space: vec4f = camera.projection * camera.view * vec4f(frag_pos, 1.0);
    return clip_space.z / clip_space.w;
}

fn linearize_depth(depth: f32) -> f32 {
    return (2.0 * camera.near_plane * camera.far_plane) /
        (camera.far_plane + camera.near_plane - depth * (camera.far_plane - camera.near_plane)) /
        camera.far_plane;
}

fn grid(frag_pos: vec3f, grid_color: vec3f, line_width: f32, scale: f32) -> vec4f {
    let coord = frag_pos.xz * scale;
    let derivative = fwidth(coord);
    let grid = max(abs(fract(coord - vec2f(0.5)) - vec2f(0.5)) - vec2f(line_width), vec2f(0.0)) / derivative;
    var color = vec4f(grid_color, 1.0 - min(min(grid.x, grid.y), 1.0));
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
        grid(frag_pos, vec3f(0.05), 0.0001, 1.0) +
        grid(frag_pos, vec3f(0.025), 0.001, 0.1);

    let view_pos = vec3f(
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