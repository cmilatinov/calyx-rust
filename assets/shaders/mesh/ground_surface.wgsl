//#include "shaders/mesh/pbr_vertex.wgsl"
//#include "shaders/mesh/ground_material.wgsl"

// The blended ground's surface, shared by its forward pass, `shaders/mesh/ground.wgsl`, and its
// geometry pass, `shaders/mesh/ground_gbuffer.wgsl`. Two masks the arena generator paints, laid
// once over the terrain by its UVs, give each layer's weight; two detail textures, tiled in
// world space, hold each layer's greyscale detail, which the layer's colour tints. World-space
// noise warps the masks' edges, picks between two differently turned detail samples so the
// tiling does not repeat visibly, and shades the ground in broad patches; the detail also acts
// as each layer's height, so layers meet along ragged, soft edges. A world-space marks texture
// multiplies the result, the hook other effects (tank tracks) darken or tint the ground through.

// Where each texture is sampled at one fragment, with the derivatives to sample it by. Image
// textures load with clamp-to-edge samplers, so the passes wrap the detail coordinates with
// `fract` and sample by the unwrapped coordinates' derivatives, which keeps the mip level
// steady across a tile's seam.
struct GroundCoords {
    // The masks' coordinates, warped by noise; `field_uv` is the unwarped one the fields'
    // crisp edges are read at.
    mask_uv: vec2f,
    field_uv: vec2f,
    mask_dx: vec2f,
    mask_dy: vec2f,
    detail_uv0: vec2f,
    detail_dx0: vec2f,
    detail_dy0: vec2f,
    detail_uv1: vec2f,
    detail_dx1: vec2f,
    detail_dy1: vec2f,
    marks_uv: vec2f,
    marks_dx: vec2f,
    marks_dy: vec2f,
    // 1 inside the marks rectangle, else 0.
    in_marks: f32,
    world: vec2f,
};

// The texels the passes sample at `GroundCoords`.
struct GroundTexels {
    mask_a: vec4f,
    mask_b: vec4f,
    // Mask B at `field_uv`.
    field: vec4f,
    // Each detail texture sampled at both detail coordinates.
    detail_a0: vec4f,
    detail_a1: vec4f,
    detail_b0: vec4f,
    detail_b1: vec4f,
    marks: vec4f,
};

struct GroundSurface {
    albedo: vec3f,
    roughness: f32,
};

// The normal the ground is lit by: the interpolated vertex `normal`, or straight up where the
// material lights it as level ground (`mask.z`).
fn ground_normal(material: GroundProperties, normal: vec3f) -> vec3f {
    return normalize(mix(normalize(normal), vec3f(0.0, 1.0, 0.0), material.mask.z));
}

fn ground_hash(p: vec2f) -> f32 {
    var q = fract(p * vec2f(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn ground_value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = ground_hash(i);
    let b = ground_hash(i + vec2f(1.0, 0.0));
    let c = ground_hash(i + vec2f(0.0, 1.0));
    let d = ground_hash(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// `octaves` octaves of value noise, each turned, in [0, 1]. Few octaves: every fragment of
// the ground evaluates several of these.
fn ground_fbm(p_in: vec2f, octaves: i32) -> f32 {
    var p = p_in;
    var sum = 0.0;
    var amplitude = 0.5;
    var total = 0.0;
    let turn = mat2x2f(0.8, 0.6, -0.6, 0.8);
    for (var i = 0; i < octaves; i++) {
        sum += amplitude * ground_value_noise(p);
        total += amplitude;
        p = turn * p * 2.03 + vec2f(17.1, 9.3);
        amplitude *= 0.5;
    }
    return sum / total;
}

// The coordinates of every texture at a fragment at `world_position`, whose terrain UV is `uv`.
// Call from uniform control flow: it takes derivatives.
fn ground_coords(material: GroundProperties, world_position: vec3f, uv: vec2f) -> GroundCoords {
    var c: GroundCoords;
    let world = world_position.xz;
    c.world = world;
    // Warp the masks by up to `blend.x` metres; V runs towards -Z.
    let warp = vec2f(
        ground_fbm(world * 0.35 + vec2f(3.1, 7.7), 2),
        ground_fbm(world * 0.35 + vec2f(11.3, 1.9), 2),
    ) - 0.5;
    let mask_size = max(material.mask.xy, vec2f(1e-3));
    c.mask_uv = uv + vec2f(warp.x, -warp.y) * 2.0 * material.blend.x / mask_size;
    c.field_uv = uv;
    c.mask_dx = dpdx(uv);
    c.mask_dy = dpdy(uv);
    let tile = max(material.detail.x, 1e-3);
    c.detail_uv0 = world / tile;
    // The second sample is larger, turned 37 degrees and shifted.
    let turn = mat2x2f(0.7986, 0.6018, -0.6018, 0.7986);
    c.detail_uv1 = turn * world / (tile * max(material.detail.y, 1e-3)) + vec2f(0.37, 0.71);
    c.detail_dx0 = dpdx(c.detail_uv0);
    c.detail_dy0 = dpdy(c.detail_uv0);
    c.detail_dx1 = dpdx(c.detail_uv1);
    c.detail_dy1 = dpdy(c.detail_uv1);
    let marks_size = material.marks_rect.zw;
    c.marks_uv = (world - material.marks_rect.xy) / max(marks_size, vec2f(1e-3));
    c.marks_dx = dpdx(c.marks_uv);
    c.marks_dy = dpdy(c.marks_uv);
    let inside = all(c.marks_uv >= vec2f(0.0)) && all(c.marks_uv <= vec2f(1.0)) &&
        all(marks_size > vec2f(0.0));
    c.in_marks = select(0.0, 1.0, inside);
    return c;
}

// One layer's colour, shaded by its detail `d` (0.5 leaves it as is).
fn ground_layer(color: vec4f, d: f32) -> vec3f {
    return color.rgb * max(1.0 + (d - 0.5) * 2.0 * color.a, 0.0);
}

fn ground_surface(material: GroundProperties, t: GroundTexels, c: GroundCoords) -> GroundSurface {
    // Pick mostly one of the two detail samples, by broad noise, so neither one's tiling reads.
    let pick = smoothstep(0.4, 0.6, ground_fbm(c.world / 9.0 + vec2f(5.2, 1.3), 2));
    let da = mix(t.detail_a0, t.detail_a1, pick);
    let db = mix(t.detail_b0, t.detail_b1, pick);
    // Detail A: grass, dirt, sand, gravel. Detail B: field soil, mud, dark grass, light grass.
    let d_grass = da.r;
    let d_dirt = da.g;
    let d_sand = da.b;
    let d_gravel = da.a;
    let d_field = db.r;
    let d_mud = db.g;
    let d_dark = db.b;
    let d_light = db.a;

    var w: array<f32, 8>;
    w[1] = t.mask_a.r;
    w[2] = t.mask_a.g;
    w[3] = t.mask_a.b;
    w[4] = t.mask_a.a;
    w[5] = t.mask_b.r;
    // Fields are laid over the blend afterwards, with crisp edges.
    w[6] = 0.0;
    w[7] = t.mask_b.b;
    w[0] = max(1.0 - (w[1] + w[2] + w[3] + w[4] + w[5] + w[6] + w[7]), 0.0);
    var d: array<f32, 8>;
    d[0] = d_grass;
    d[1] = d_dark;
    d[2] = d_light;
    d[3] = d_dirt;
    d[4] = d_sand;
    d[5] = d_gravel;
    d[6] = d_field;
    d[7] = d_mud;
    // Height blend: each layer's weight raised by its detail; only the layers within `band` of
    // the highest show, so layers meet along their detail's shapes.
    let band = max(material.blend.y, 1e-3);
    var h: array<f32, 8>;
    var top = -1.0;
    for (var i = 0; i < 8; i++) {
        h[i] = select(-1.0, w[i] + d[i] * band, w[i] > 0.001);
        top = max(top, h[i]);
    }
    var total = 0.0;
    for (var i = 0; i < 8; i++) {
        h[i] = max(h[i] - top + band, 0.0);
        total += h[i];
    }
    let rows_along_x = t.field.a > 0.5;
    let across = select(c.world.x, c.world.y, rows_along_x);
    let wobble = (d_field - 0.5) * 0.06;
    let row_half = material.blend.z * 0.5;
    let row = 1.0 - smoothstep(row_half - 0.03, row_half + 0.03, abs(fract(across) - 0.5) + wobble);
    let field = mix(ground_layer(material.field_color, d_field),
        ground_layer(material.crop_color, d_grass), row);

    var albedo = ground_layer(material.grass_color, d_grass) * h[0] +
        ground_layer(material.grass_dark_color, d_dark) * h[1] +
        ground_layer(material.grass_light_color, d_light) * h[2] +
        ground_layer(material.dirt_color, d_dirt) * h[3] +
        ground_layer(material.sand_color, d_sand) * h[4] +
        ground_layer(material.gravel_color, d_gravel) * h[5] +
        ground_layer(material.mud_color, d_mud) * h[7];
    albedo /= max(total, 1e-4);
    // A field's edge follows its rectangle, sharp at the gameplay camera.
    let in_field = smoothstep(0.4, 0.6, t.field.g);
    albedo = mix(albedo, field, in_field);
    // Broad patches of light and shade.
    let patches = ground_fbm(c.world / max(material.detail.z, 1e-3) + vec2f(2.7, 8.1), 3) - 0.5;
    albedo *= 1.0 + patches * 2.0 * material.detail.w;
    // The marks hook: multiplies the ground inside its rectangle.
    albedo *= mix(vec3f(1.0), t.marks.rgb, c.in_marks);
    var surface: GroundSurface;
    surface.albedo = albedo;
    surface.roughness = mix(material.roughness, material.blend.w,
        h[7] / max(total, 1e-4) * (1.0 - in_field));
    return surface;
}
