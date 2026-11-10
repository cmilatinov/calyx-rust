// The scene's grass field, group 3 of `shaders/mesh/grass_gbuffer.wgsl` and group 0 binding 8 of
// `shaders/mesh/grass.wgsl`: the wind and what moves through the grass, as `ComponentGrassField`
// hands them to the renderer.

// The most benders the field holds, as `MAX_GRASS_BENDERS` on the Rust side.
const MAX_GRASS_BENDERS: u32 = 16u;

struct GrassField {
    // Seconds on the renderer's clock.
    time: f32,
    // Tip sway and bend in metres per metre of height, and the sway's speed in radians per second.
    sway: f32,
    sway_speed: f32,
    bend: f32,
    // The height full sway and bend are reached at, in metres.
    height: f32,
    // How many of `benders` are in use.
    bender_count: u32,
    // How far the grass droops down and outward from its mesh's origin, in metres per metre of
    // height, with the square of the height.
    droop: f32,
    _padding: f32,
    // Each bender's ground position in xy, its bend radius in z and its reveal radius in w.
    benders: array<vec4f, MAX_GRASS_BENDERS>,
};

// The opacity the grass fades to around a bender that reveals: `GRASS_REVEAL_ALPHA` in the ring
// round it, enough to still read as grass, and `GRASS_CORE_ALPHA` over the tank itself, so it
// reads clearly through the grass in its own colours. The transparent pass blends only the
// nearest faded blade over each pixel, so these are the whole of the grass's tint, however many
// blades stack there.
const GRASS_REVEAL_ALPHA: f32 = 0.55;
const GRASS_CORE_ALPHA: f32 = 0.15;

// How thick a blade stays at its tip, as a fraction of its modelled thickness, and how far along
// its length, as a fraction of the field's height, it starts thinning towards it.
const GRASS_TIP_THICKNESS: f32 = 0.08;
const GRASS_TAPER_START: f32 = 0.35;

// How dark a blade is at its root, as a fraction of its colour, brightening to the full colour
// at its tip, so the arc of a blade reads from above.
const GRASS_ROOT_SHADE: f32 = 0.7;

// How far the grass vertex at `world`, of a mesh standing at `root`, moves, the more the higher
// above its root it is:
// - it droops down and outward from its root by the field's droop;
// - it sways in the wind along +X, out of step with its neighbours;
// - it bends away from the benders near it, the closer the more, and a little lower, so blades
//   part rather than stretch.
fn grass_displacement(field: GrassField, root: vec3f, world: vec3f) -> vec3f {
    let height = max(world.y - root.y, 0.0);
    let rise = height / max(field.height, 0.01);
    let weight = rise * rise;
    let phase = world.x * 0.9 + world.z * 0.6;
    let sway = sin(field.time * field.sway_speed + phase)
        + 0.35 * sin(field.time * field.sway_speed * 2.3 + phase * 1.7);
    var offset = vec3f(sway * field.sway * field.height * weight, 0.0, 0.0);
    // The droop: out from the mesh's origin and down, the more the higher, so a straight blade
    // leaning out arcs over.
    let out = world.xz - root.xz;
    let outward = select(vec2f(0.0), out / length(out), length(out) > 1e-4);
    let droop = field.droop * height * rise;
    offset += vec3f(outward.x * droop, -droop * 0.6, outward.y * droop);
    var push = vec2f(0.0);
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        let away = world.xz - bender.xy;
        let distance = length(away);
        if distance < bender.z && bender.z > 0.0 {
            let closeness = 1.0 - distance / bender.z;
            let direction = select(vec2f(1.0, 0.0), away / distance, distance > 1e-4);
            push += direction * closeness * sqrt(closeness);
        }
    }
    let strength = min(length(push), 1.0);
    if strength > 0.0 {
        let bend = normalize(push) * strength * field.bend * field.height;
        offset += vec3f(bend.x, -0.45 * strength * field.height, bend.y) * weight;
    }
    return offset;
}

// The shading normal of a grass blade whose face points along `normal`: bent most of the way up,
// as foliage is lit, so a blade's underside and the faces turned from the sun are lit like the
// sward round them rather than going black in the shade of the blades over them.
fn grass_normal(normal: vec3f) -> vec3f {
    return normalize(mix(normalize(normal), vec3f(0.0, 1.0, 0.0), 0.65));
}

// The position of a grass mesh's vertex at `local`, thinned towards its tip. A grass blade is
// modelled lying flat: its outline in XZ, its length running along -Z from its root at the
// origin, and its thickness rising along +Y from 0. Over the outer part of its length the blade
// thins down onto its underside, to `GRASS_TIP_THICKNESS` of its thickness, so its tip ends in a
// thin rounded edge rather than a blunt cap.
fn grass_taper(field: GrassField, local: vec3f) -> vec3f {
    let along = max(-local.z, 0.0) / max(field.height, 0.01);
    let thickness = mix(1.0, GRASS_TIP_THICKNESS, smoothstep(GRASS_TAPER_START, 1.0, along));
    return vec3f(local.x, local.y * thickness, local.z);
}

// How far up its blade the grass vertex at `world`, of a mesh standing at `root`, is, from 0 at
// its root to 1 at the field's height.
fn grass_rise(field: GrassField, root: vec3f, world: vec3f) -> f32 {
    return clamp((world.y - root.y) / max(field.height, 0.01), 0.0, 1.0);
}

// The colour of grass `rise` up its blade, of colour `albedo`: darker towards the root.
fn grass_shade(albedo: vec3f, rise: f32) -> vec3f {
    return albedo * mix(GRASS_ROOT_SHADE, 1.0, smoothstep(0.0, 0.9, rise));
}

// The opacity of the grass at `world` around the benders that reveal: 1 outside their reveal
// radius, easing over its outer two thirds to `GRASS_REVEAL_ALPHA`, and from there, over the
// inner 0.6 of the radius, down to `GRASS_CORE_ALPHA` over the bender and the tank round it.
fn grass_alpha(field: GrassField, world: vec3f) -> f32 {
    var alpha = 1.0;
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        if bender.w > 0.0 {
            let distance = length(world.xz - bender.xy);
            let ring = mix(1.0, GRASS_REVEAL_ALPHA, 1.0 - smoothstep(bender.w * 0.35, bender.w, distance));
            let core = mix(
                ring,
                GRASS_CORE_ALPHA,
                1.0 - smoothstep(bender.w * 0.35, bender.w * 0.6, distance),
            );
            alpha = min(alpha, core);
        }
    }
    return alpha;
}

// Whether the grass at `world` draws ink: not near a bender that reveals, out to a quarter past
// its reveal radius, so where the opaque grass meets the faded grass no ink line cuts the blades.
fn grass_inked(field: GrassField, world: vec3f) -> bool {
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        if bender.w > 0.0 && length(world.xz - bender.xy) < bender.w * 1.25 {
            return false;
        }
    }
    return true;
}
