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
    // How far the grass droops down and outward from its mesh's origin, in metres per metre of
    // height, with the square of the height.
    droop: f32,
    _padding: f32,
    // Each bender's ground position in xy, its bend radius in z and its reveal radius in w.
    benders: array<vec4f, MAX_GRASS_BENDERS>,
};

// How far the grass vertex at `world`, of a mesh standing at `root`, moves, the more the higher
// above its root it is:
// - it droops down and outward from its root by the field's droop;
// - it sways in the wind along +X, out of step with its neighbours;
// - it bends away from the benders near it, the closer the more, and a little lower, so blades
//   part rather than stretch;
// - within a bender's reveal radius the grass lies down out of the way: the vertex moves out
//   towards the edge of the radius and down towards the ground, fully in the inner half and
//   easing off over the outer half, opening a clearing what reveals it stands in. The grass
//   stays opaque, so nothing dithers or flickers.
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
    var clear = 0.0;
    var clear_push = vec2f(0.0);
    for (var i = 0u; i < min(field.bender_count, MAX_GRASS_BENDERS); i++) {
        let bender = field.benders[i];
        let away = world.xz - bender.xy;
        let distance = length(away);
        let direction = select(vec2f(1.0, 0.0), away / distance, distance > 1e-4);
        if distance < bender.z && bender.z > 0.0 {
            let closeness = 1.0 - distance / bender.z;
            push += direction * closeness * sqrt(closeness);
        }
        if bender.w > 0.0 {
            let amount = 1.0 - smoothstep(bender.w * 0.5, bender.w, distance);
            if amount > clear {
                clear = amount;
                clear_push = direction * max(bender.w - distance, 0.0);
            }
        }
    }
    let strength = min(length(push), 1.0);
    if strength > 0.0 {
        let bend = normalize(push) * strength * field.bend * field.height;
        offset += vec3f(bend.x, -0.45 * strength * field.height, bend.y) * weight;
    }
    if clear > 0.0 {
        // The higher the vertex, the further it leans out and the lower it lies, so a blade
        // rooted in the clearing folds over outward from its root.
        let lean = min(rise, 1.0);
        offset += vec3f(clear_push.x, 0.0, clear_push.y) * clear * lean;
        offset.y -= height * 0.95 * clear;
    }
    return offset;
}
