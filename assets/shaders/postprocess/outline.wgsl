//#include "shaders/common/fullscreen_pass.wgsl"
//#include "shaders/deferred/gbuffer_encoding.wgsl"
//#include "shaders/postprocess/camera_planes.wgsl"

// Ink outlines on display-linear color, after colour grading and before FXAA.
//
// Each surface's material names an outline style in the G-buffer's material alpha: 0 for none,
// N for `ink.styles[N - 1]`. An edge between two neighbouring pixels takes the nearer surface's
// style. A nearer surface of style 0 hides the farther surface's ink instead of inheriting it;
// a style past the end of the list draws nothing. Edges are:
//
// - silhouettes, at the style's `width` in full ink: a surface against the background, two
//   surfaces of different styles, or a jump in distance above the silhouette threshold (one
//   surface in front of another);
// - inner lines, at its `inner_width` and `inner_opacity`: a smaller jump in distance, or
//   geometric normals turning by more than the normal threshold (a crease).
//
// A jump in distance is measured against what the surfaces on either side predict, so planes
// at any angle draw nothing. Every pixel within an edge's reach, along a row, a column or a
// diagonal, takes that edge's ink, so a line keeps its width in pixels at any distance. A line
// straddles its edge: of `width` pixels, the nearer side takes `ceil(width / 2)`.
//
// The edges come from the G-buffer, before any forward draw, skybox or particle writes the scene
// depth, so those draw no lines; where one of them lies in front of the G-buffer surface in the
// scene depth, it hides the line there.

// The most styles the uniform holds, as `MAX_OUTLINE_STYLES` on the Rust side.
const MAX_STYLES: u32 = 8u;

struct InkStyle {
    // Display-linear ink colour in rgb, opacity in a.
    color: vec4f,
    // Silhouette and inner line widths, in whole pixels from 1 to 8.
    width: u32,
    inner_width: u32,
    // How much of the ink an inner line takes, relative to a silhouette.
    inner_opacity: f32,
    _padding: f32,
};

struct InkOutlineUniform {
    // The camera, to turn depth into distance and pixels into rays.
    camera: CameraPlanes,
    // How many of `styles` are in use.
    style_count: u32,
    // The widest line of any style, in whole pixels: how far a pixel looks for edges.
    max_width: u32,
    // Smallest relative jump in distance that is an inner line, and that is a silhouette.
    depth_threshold: f32,
    silhouette_threshold: f32,
    // Cosine of the smallest angle between normals that is a crease.
    normal_cos: f32,
    styles: array<InkStyle, MAX_STYLES>,
};

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var gbuffer_depth: texture_depth_2d;

@group(0) @binding(2)
var normals_texture: texture_2d<f32>;

@group(0) @binding(3)
var material_texture: texture_2d<f32>;

@group(0) @binding(4)
var scene_depth: texture_depth_2d;

@group(0) @binding(5)
var<uniform> ink: InkOutlineUniform;

struct Surface {
    // 1 / view depth, which is linear in screen space across a plane.
    inverse_depth: f32,
    // View-space geometric normal.
    normal: vec3f,
    // Whether an opaque PBR surface was drawn there.
    covered: bool,
    // The material's outline style: 0 for none, N for `ink.styles[N - 1]`.
    style: u32,
};

fn surface_at(pixel: vec2i) -> Surface {
    let size = vec2i(textureDimensions(gbuffer_depth));
    let at = clamp(pixel, vec2i(0), size - vec2i(1));
    let depth = textureLoad(gbuffer_depth, at, 0);
    var surface: Surface;
    surface.covered = depth < 1.0;
    surface.inverse_depth = camera_inverse_depth(ink.camera, depth);
    surface.normal = gbuffer_decode_normal(textureLoad(normals_texture, at, 0).ba);
    surface.style = select(0u, u32(round(textureLoad(material_texture, at, 0).a * 255.0)), surface.covered);
    return surface;
}

// The jump in distance between `b` and `c`, neighbours on a line through `a`, `b`, `c`, `d`,
// that the surfaces on either side do not predict, relative to the nearer one. Each side
// extrapolates its own surface across the edge, linearly in inverse depth, so a plane at any
// angle predicts its next pixel exactly; the smaller error of the two wins, so a crease where
// only the slope changes stays small. A side whose outer pixel holds no surface compares the two
// pixels directly.
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

struct Edge {
    // The style the edge draws in: 0 for no edge.
    style: u32,
    // A silhouette, rather than an inner line.
    silhouette: bool,
};

// The edge between neighbours `b` and `c`, seen along `ray`.
//
// Where both surfaces turn edge-on to the camera, as a rounded shape does towards its
// silhouette, the distance bends faster than either side predicts, so the depth thresholds grow
// as the more camera-facing of the two turns away, up to five times.
fn edge_between(a: Surface, b: Surface, c: Surface, d: Surface, ray: vec3f) -> Edge {
    var edge: Edge;
    edge.style = 0u;
    edge.silhouette = false;
    let b_nearer = b.inverse_depth >= c.inverse_depth;
    let style = select(c.style, b.style, b_nearer);
    if style == 0u || style > ink.style_count {
        return edge;
    }
    if b.covered != c.covered || b.style != c.style {
        edge.style = style;
        edge.silhouette = true;
        return edge;
    }
    let facing = max(abs(dot(b.normal, ray)), abs(dot(c.normal, ray)));
    let jump = depth_jump(a, b, c, d) * max(facing, 0.2);
    if jump > ink.silhouette_threshold {
        edge.style = style;
        edge.silhouette = true;
    } else if jump > ink.depth_threshold || dot(b.normal, c.normal) < ink.normal_cos {
        edge.style = style;
    }
    return edge;
}

// The ink at `pixel`: the colour of the strongest line within reach in rgb, how much of it the
// pixel takes in a (0 for none).
fn ink_at(pixel: vec2i) -> vec4f {
    let receiver = surface_at(pixel);
    let widest_reach = (i32(ink.max_width) + 1) / 2;
    // The view-space direction through the pixel.
    let size = vec2f(textureDimensions(gbuffer_depth));
    let ray = normalize(camera_ray(ink.camera, camera_ndc((vec2f(pixel) + 0.5) / size)));
    var best = vec4f(0.0);
    for (var k = 0; k < 4; k++) {
        // Right, down, and the two diagonals, whose steps are sqrt(2) pixels long.
        let direction = vec2i(select(1, 0, k == 1), select(select(0, 1, k == 1 || k == 2), -1, k == 3));
        let step_length = select(1.0, sqrt(2.0), k >= 2);
        // The edge between the pixels i and i + 1 steps along lies between b and c, with a and d
        // beyond them; each step loads one new pixel.
        var a = surface_at(pixel - direction * (widest_reach + 1));
        var b = surface_at(pixel - direction * widest_reach);
        var c = surface_at(pixel - direction * (widest_reach - 1));
        for (var i = -widest_reach; i < widest_reach; i++) {
            let d = surface_at(pixel + direction * (i + 2));
            // This pixel is on b's side when i >= 0, else on c's, `steps` pixels from the one
            // beside the edge.
            let on_b_side = i >= 0;
            let steps = f32(select(-(i + 1), i, on_b_side));
            let distance = (steps + 0.5) * step_length;
            if distance < f32(widest_reach) {
                let edge = edge_between(a, b, c, d, ray);
                if edge.style != 0u {
                    let style = ink.styles[edge.style - 1u];
                    let width = i32(select(style.inner_width, style.width, edge.silhouette));
                    let b_nearer = b.inverse_depth >= c.inverse_depth;
                    let reach = select(width / 2, (width + 1) / 2, on_b_side == b_nearer);
                    let amount = style.color.a * select(style.inner_opacity, 1.0, edge.silhouette);
                    // A nearby edge can reach beyond the two pixels that formed it.
                    // Do not dilate its ink across a nearer, unoutlined PBR surface
                    // such as an emissive projectile passing over outlined cover.
                    let occluded = receiver.covered && receiver.style == 0u
                        && receiver.inverse_depth > max(b.inverse_depth, c.inverse_depth) * 1.0001;
                    if !occluded && distance < f32(reach) && amount > best.a {
                        best = vec4f(style.color.rgb, amount);
                    }
                }
            }
            a = b;
            b = c;
            c = d;
        }
    }
    return best;
}

@fragment
fn fs_main(in: FullscreenVertexOut) -> @location(0) vec4f {
    let pixel = vec2i(in.position.xy);
    let texel = textureLoad(input_texture, pixel, 0);
    // A forward draw or a particle in front of the G-buffer surface hides the line, as it hides
    // the surface.
    let surface = camera_inverse_depth(ink.camera, textureLoad(gbuffer_depth, pixel, 0));
    let drawn = camera_inverse_depth(ink.camera, textureLoad(scene_depth, pixel, 0));
    let hidden = drawn > surface * 1.01;
    let line = ink_at(pixel);
    let amount = select(saturate(line.a), 0.0, hidden);
    return vec4f(mix(texel.rgb, line.rgb, amount), mix(texel.a, 1.0, amount));
}
