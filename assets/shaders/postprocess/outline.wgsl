//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"

// Ink outlines on display-linear color, after colour grading and before FXAA.
//
// An edge lies between two neighbouring pixels of the G-buffer where one
// holds an opaque PBR surface and the other none (a silhouette against the
// background), where the distance jumps by more than the surfaces on either
// side predict (a silhouette against a farther surface), or where the
// geometric normals turn by more than the normal threshold (a crease). Every
// pixel within the line's reach of an edge, along a row, a column or a
// diagonal, takes the ink colour, so the line keeps its width in pixels at
// any distance. The line straddles the edge: of `width` pixels, the nearer
// side takes `ceil(width / 2)` and the farther side the rest.
//
// The edges come from the G-buffer's own depth, before any forward draw,
// skybox or particle writes the scene depth, so those draw no lines; where
// one of them lies in front of the G-buffer surface in the scene depth, it
// hides the line there.

struct OutlineUniform {
    // Display-linear ink colour in rgb, opacity in a.
    color: vec4f,
    // The camera's clip planes, to turn depth into view distance.
    near_plane: f32,
    far_plane: f32,
    // Line width in whole pixels, 1 to 8.
    width: u32,
    // Smallest relative jump in distance that is an edge.
    depth_threshold: f32,
    // Cosine of the smallest angle between normals that is an edge.
    normal_cos: f32,
    // Tangents of half the horizontal and vertical fields of view.
    tan_half_fov: vec2f,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var gbuffer_depth: texture_depth_2d;

@group(0) @binding(2)
var normals_texture: texture_2d<f32>;

@group(0) @binding(3)
var scene_depth: texture_depth_2d;

@group(0) @binding(4)
var<uniform> outline: OutlineUniform;

struct Surface {
    // 1 / view depth, which is linear in screen space across a plane.
    inverse_depth: f32,
    // View-space geometric normal.
    normal: vec3f,
    // Whether an opaque PBR surface was drawn there.
    covered: bool,
};

// 1 / view depth of a depth-buffer value (`perspective_lh` with OpenGL's
// [-1, 1] depth range, of which the buffer keeps [0, 1]): affine in the
// depth-buffer value.
fn inverse_depth(depth: f32) -> f32 {
    let near = outline.near_plane;
    let far = outline.far_plane;
    return (far + near - depth * (far - near)) / (2.0 * near * far);
}

fn surface_at(pixel: vec2i) -> Surface {
    let size = vec2i(textureDimensions(gbuffer_depth));
    let at = clamp(pixel, vec2i(0), size - vec2i(1));
    let depth = textureLoad(gbuffer_depth, at, 0);
    var surface: Surface;
    surface.covered = depth < 1.0;
    surface.inverse_depth = inverse_depth(depth);
    surface.normal = gbuffer_decode_normal(textureLoad(normals_texture, at, 0).ba);
    return surface;
}

// The jump in distance between `b` and `c`, neighbours on a line through
// `a`, `b`, `c`, `d`, that the surfaces on either side do not predict,
// relative to the nearer one. Each side extrapolates its own surface across
// the edge, linearly in inverse depth, so a plane at any angle predicts its
// next pixel exactly and draws no line; the smaller error of the two wins,
// so a crease where only the slope changes stays below a jump. A side whose
// outer pixel holds no surface compares the two pixels directly.
fn depth_jump(a: Surface, b: Surface, c: Surface, d: Surface) -> f32 {
    var from_b = abs(c.inverse_depth - b.inverse_depth);
    if a.covered {
        from_b = abs(c.inverse_depth - (2.0 * b.inverse_depth - a.inverse_depth));
    }
    var from_c = abs(b.inverse_depth - c.inverse_depth);
    if d.covered {
        from_c = abs(b.inverse_depth - (2.0 * c.inverse_depth - d.inverse_depth));
    }
    return min(from_b, from_c) / max(b.inverse_depth, c.inverse_depth);
}

// Whether an edge lies between neighbours `b` and `c`, seen along `ray`.
//
// Where both surfaces turn edge-on to the camera, as a rounded shape does
// towards its silhouette, the distance changes steeply from pixel to pixel
// and bends faster than either side predicts, so the depth threshold grows
// as the more camera-facing of the two turns away, up to five times.
fn is_edge(a: Surface, b: Surface, c: Surface, d: Surface, ray: vec3f) -> bool {
    if b.covered != c.covered {
        return true;
    }
    if !b.covered {
        return false;
    }
    let facing = max(abs(dot(b.normal, ray)), abs(dot(c.normal, ray)));
    return depth_jump(a, b, c, d) > outline.depth_threshold / max(facing, 0.2)
        || dot(b.normal, c.normal) < outline.normal_cos;
}

// Whether `pixel` lies on a line: within its side's reach of an edge along
// a row, a column or a diagonal.
fn on_line(pixel: vec2i) -> bool {
    let width = i32(outline.width);
    let near_reach = (width + 1) / 2;
    let far_reach = width / 2;
    // The view-space direction through the pixel, +z away from the camera.
    let size = vec2f(textureDimensions(gbuffer_depth));
    let centre = vec2f(pixel) + 0.5;
    let ndc = vec2f(centre.x / size.x * 2.0 - 1.0, 1.0 - centre.y / size.y * 2.0);
    let ray = normalize(vec3f(ndc * outline.tan_half_fov, 1.0));
    var found = false;
    for (var k = 0; k < 4; k++) {
        // Right, down, and the two diagonals, whose steps are sqrt(2) pixels
        // long.
        let direction = vec2i(select(1, 0, k == 1), select(select(0, 1, k == 1 || k == 2), -1, k == 3));
        let step_length = select(1.0, sqrt(2.0), k >= 2);
        // The edge between the pixels i and i + 1 steps along lies between
        // b and c, with a and d beyond them; each step loads one new pixel.
        var a = surface_at(pixel - direction * (near_reach + 1));
        var b = surface_at(pixel - direction * near_reach);
        var c = surface_at(pixel - direction * (near_reach - 1));
        for (var i = -near_reach; i < near_reach; i++) {
            let d = surface_at(pixel + direction * (i + 2));
            // This pixel is on b's side when i >= 0, else on c's, `steps`
            // pixels from the one beside the edge.
            let on_b_side = i >= 0;
            let steps = select(-(i + 1), i, on_b_side);
            let b_nearer = b.inverse_depth >= c.inverse_depth;
            let reach = select(far_reach, near_reach, on_b_side == b_nearer);
            if (f32(steps) + 0.5) * step_length < f32(reach) {
                found = found || is_edge(a, b, c, d, ray);
            }
            a = b;
            b = c;
            c = d;
        }
    }
    return found;
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    // A forward draw or a particle in front of the G-buffer surface hides
    // the line, as it hides the surface.
    let surface = inverse_depth(textureLoad(gbuffer_depth, pixel, 0));
    let drawn = inverse_depth(textureLoad(scene_depth, pixel, 0));
    let hidden = drawn > surface * 1.01;
    let amount = select(0.0, saturate(outline.color.a), on_line(pixel) && !hidden);
    return vec4f(mix(texel.rgb, outline.color.rgb, amount), mix(texel.a, 1.0, amount));
}
