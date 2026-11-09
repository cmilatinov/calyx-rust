// The scene's grass field, group 3 of `shaders/mesh/grass_gbuffer.wgsl`: the wind and what moves
// through the grass, as `ComponentGrassField` hands them to the renderer.

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
    _padding: vec2f,
    // Each bender's ground position in xy, its bend radius in z and its reveal radius in w.
    benders: array<vec4f, MAX_GRASS_BENDERS>,
};

// How far the grass vertex at `world`, of a mesh standing at `root`, moves: it sways in the wind
// along +X, out of step with its neighbours, and bends away from the benders near it, the more
// the higher above its root it is and the closer to a bender. Bending away also lowers it a
// little, so blades part rather than stretch.
fn grass_displacement(field: GrassField, root: vec3f, world: vec3f) -> vec3f {
    let rise = max(world.y - root.y, 0.0) / max(field.height, 0.01);
    let weight = rise * rise;
    let phase = world.x * 0.9 + world.z * 0.6;
    let sway = sin(field.time * field.sway_speed + phase) + 0.35 * sin(field.time * field.sway_speed * 2.3 + phase * 1.7);
    var offset = vec3f(sway * field.sway * field.height * weight, 0.0, 0.0);
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

// How much the grass at `world` thins out around the benders that reveal: 0 outside their reveal
// radius, easing up over its outer two thirds to the most it thins.
fn grass_reveal(field: GrassField, world: vec3f) -> f32 {
    var reveal = 0.0;
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        if bender.w > 0.0 {
            let distance = length(world.xz - bender.xy);
            reveal = max(reveal, 1.0 - smoothstep(bender.w * 0.35, bender.w, distance));
        }
    }
    return reveal * 0.8;
}

// Whether grass thinned by `reveal` leaves out the pixel at `pixel`. Interleaved gradient noise
// spreads the pixels left out finely and without the grid a small ordered pattern shows, so the
// thinning reads as a soft fade once anti-aliased.
fn grass_reveals(reveal: f32, pixel: vec2f) -> bool {
    if reveal <= 0.0 {
        return false;
    }
    let noise = fract(52.9829189 * fract(dot(floor(pixel), vec2f(0.06711056, 0.00583715))));
    return noise < reveal;
}
