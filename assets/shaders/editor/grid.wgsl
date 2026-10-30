//#include "shaders/common/fullscreen.wgsl"
//#include "shaders/common/camera.wgsl"

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

// Interpolating the near and far points across the full-screen triangle is
// exact: a point on a fixed clip-space depth unprojects affinely in screen xy.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let pos = fullscreen_vertex(index).position.xy;
    var out: VertexOut;
    out.position = vec4f(pos, 0.0, 1.0);
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

// One grid layer: lines at every integer of `frag_pos.xz * scale`, `color`
// at `opacity` coverage. A line is `line_width` wide in grid units plus
// `pixel_width` pixels in screen space, with a one pixel anti-aliased edge.
// Each line direction fades out as its lines close in to a few pixels apart,
// far away or at a grazing angle, so they never merge into a solid sheet. The
// axes, the lines through zero, draw at full coverage in their own colors.
fn grid(
    frag_pos: vec3f,
    color: vec3f,
    opacity: f32,
    line_width: f32,
    pixel_width: f32,
    scale: f32,
) -> vec4f {
    let coord = frag_pos.xz * scale;
    let derivative = fwidth(coord);
    let pixels = max(abs(fract(coord - vec2f(0.5)) - vec2f(0.5)) - vec2f(line_width), vec2f(0.0)) / derivative;
    let edge = vec2f(1.0) - clamp(pixels - vec2f(pixel_width), vec2f(0.0), vec2f(1.0));
    let density = vec2f(1.0) - smoothstep(vec2f(0.1), vec2f(0.4), derivative);
    let lines = edge * density;
    let coverage = max(lines.x, lines.y) * opacity;
    // The line through zero is an axis. It keeps full coverage at any density.
    let axis = edge * step(abs(coord), vec2f(0.5));
    if (axis.x > 0.0) {
        return vec4f(0.1, 0.1, 1.0, max(axis.x, coverage));
    } else if (axis.y > 0.0) {
        return vec4f(1.0, 0.1, 0.1, max(axis.y, coverage));
    }
    return vec4f(color, coverage);
}

// Straight-alpha `top` over `bottom`, both with alpha in [0, 1].
fn over(top: vec4f, bottom: vec4f) -> vec4f {
    let alpha = top.a + bottom.a * (1.0 - top.a);
    if (alpha <= 0.0) {
        return vec4f(0.0);
    }
    let color = (top.rgb * top.a + bottom.rgb * bottom.a * (1.0 - top.a)) / alpha;
    return vec4f(color, alpha);
}

@fragment
fn fs_main(in: VertexOut) -> FragmentOut {
    let t = -in.near_point.y / (in.far_point.y - in.near_point.y);
    if (t < 0.0) {
        discard;
    }

    var out: FragmentOut;
    let frag_pos = in.near_point + t * (in.far_point - in.near_point);
    // The major lines and axes cover the minor ones, so where both layers
    // draw, which is every tenth line and both axes, coverage stays at most
    // one. A plain sum reached two there, and the premultiplied composite
    // then weighted the scene below by a negative factor.
    // The major lines, every tenth one, stand out from the minor ones by a
    // lighter color, higher coverage and an extra half pixel of width, all in
    // range. The axes are the most prominent: full coverage in color.
    let minor = grid(frag_pos, vec3f(0.05), 0.5, 0.0001, 0.0, 1.0);
    let major = grid(frag_pos, vec3f(0.15), 0.9, 0.001, 0.5, 0.1);
    out.color = clamp(over(major, minor), vec4f(0.0), vec4f(1.0));

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