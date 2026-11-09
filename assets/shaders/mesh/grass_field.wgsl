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

// The least opacity grass fades to around a bender that reveals: enough to still read as grass
// while what stands in it reads clearly through it.
const GRASS_REVEAL_ALPHA: f32 = 0.35;

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

// How far the grass at `world` fades towards `GRASS_REVEAL_ALPHA` around the benders that reveal,
// from 0 outside their reveal radius, easing in over its outer two thirds, to 1.
fn grass_reveal(field: GrassField, world: vec3f) -> f32 {
    var reveal = 0.0;
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        if bender.w > 0.0 {
            let distance = length(world.xz - bender.xy);
            reveal = max(reveal, 1.0 - smoothstep(bender.w * 0.35, bender.w, distance));
        }
    }
    return reveal;
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

// The opacity of grass `reveal` has faded.
fn grass_alpha(reveal: f32) -> f32 {
    return mix(1.0, GRASS_REVEAL_ALPHA, reveal);
}
