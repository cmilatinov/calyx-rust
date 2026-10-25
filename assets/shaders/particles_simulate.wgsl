// Particle kernels. The simulation (`update_main`, `scatter_main`) advances
// each system's particle buffer once per frame; every view then evaluates
// (`evaluate_main`), sorts (`sort_main`) and gathers (`gather_main`) those
// particles into its own instance buffer.
//
// Each kernel binds only its own resources from group 0, so every binding
// number is used by one role.

//#include "shaders/curve.wgsl"
//#include "shaders/gradient.wgsl"

const WORKGROUP_SIZE: u32 = 64u;
// Shortest lifetime a particle can have, as on the CPU.
const MIN_PARTICLE_LIFETIME: f32 = 0.01;
// Sort key of a slot that draws nothing: after every live particle.
const DEAD_KEY: f32 = 3.0e38;

struct Particle {
    position: vec3f,
    age: f32,
    velocity: vec3f,
    lifetime: f32,
    acceleration: vec3f,
    size_offset: f32,
};

struct Spawn {
    particle: Particle,
    // Slot the particle goes into.
    slot: u32,
    // Index of the first of this frame's steps the particle lives through.
    first_step: u32,
    _padding: vec2u,
};

struct SimulationParams {
    slot_count: u32,
    step_count: u32,
    spawn_count: u32,
    _padding: u32,
};

struct RenderParams {
    emitter: mat4x4f,
    camera_position: vec3f,
    local_space: u32,
    slot_count: u32,
    sort_count: u32,
    // Index of the `vec4f` of `module_keys` the gradient's keys start at.
    gradient_base: u32,
    _padding: u32,
    // The size module's curve and the color module's gradient in
    // `module_keys`.
    size_curve: CurveRange,
    color_gradient: GradientRange,
};

struct SortParams {
    // Distance between the compared elements.
    j: u32,
    // Size of the bitonic sequences being merged.
    k: u32,
    _padding: vec2u,
};

struct SortKey {
    key: f32,
    index: u32,
};

struct Instance {
    position_size: vec4f,
    color: vec4f,
};

// Simulation.
@group(0) @binding(0) var<uniform> simulation: SimulationParams;
@group(0) @binding(1) var<storage, read> steps: array<f32>;
@group(0) @binding(2) var<storage, read> spawns: array<Spawn>;
@group(0) @binding(3) var<storage, read_write> particles: array<Particle>;

// View.
@group(0) @binding(4) var<uniform> view: RenderParams;
@group(0) @binding(5) var<storage, read> source_particles: array<Particle>;
@group(0) @binding(6) var<storage, read_write> keys: array<SortKey>;
@group(0) @binding(7) var<storage, read_write> evaluated: array<Instance>;
@group(0) @binding(8) var<storage, read_write> instances: array<Instance>;
@group(0) @binding(9) var<uniform> sort: SortParams;
// The system's curve keys, one `vec4f` each as `CurveKey` lays them out,
// then from `view.gradient_base` its gradient keys, two `vec4f`s each as
// `GradientKey` lays them out. One buffer for both keeps the kernels within
// eight storage buffers per stage.
@group(0) @binding(10) var<storage, read> module_keys: array<vec4f>;

// The key source of `curve.wgsl`.
fn curve_key(index: u32) -> CurveKey {
    let key = module_keys[index];
    return CurveKey(key.x, key.y, bitcast<u32>(key.z), 0u);
}

// The key source of `gradient.wgsl`.
fn gradient_key(index: u32) -> GradientKey {
    let at = view.gradient_base + 2u * index;
    return GradientKey(module_keys[at], module_keys[at + 1u].x, 0u, 0u, 0u);
}

fn thread_index(gid: vec3u, workgroups: vec3u) -> u32 {
    return gid.y * workgroups.x * WORKGROUP_SIZE + gid.x;
}

fn is_alive(particle: Particle) -> bool {
    return particle.age < max(particle.lifetime, MIN_PARTICLE_LIFETIME);
}

// Runs this frame's steps from `first` on: `advance_age`, then the forces
// module's semi-implicit Euler, until the particle dies.
fn advance(particle: ptr<function, Particle>, first: u32) {
    for (var step = first; step < simulation.step_count; step++) {
        if !is_alive(*particle) {
            break;
        }
        let delta_time = steps[step];
        (*particle).age += delta_time;
        (*particle).velocity += (*particle).acceleration * delta_time;
        (*particle).position += (*particle).velocity * delta_time;
    }
}

// Advances every slot through every step of the frame.
@compute @workgroup_size(64)
fn update_main(
    @builtin(global_invocation_id) gid: vec3u,
    @builtin(num_workgroups) workgroups: vec3u,
) {
    let index = thread_index(gid, workgroups);
    if index >= simulation.slot_count {
        return;
    }
    var particle = particles[index];
    advance(&particle, 0u);
    particles[index] = particle;
}

// Writes each particle spawned this frame into its slot, advanced through the
// steps after the one it spawned in. Runs after `update_main`.
@compute @workgroup_size(64)
fn scatter_main(
    @builtin(global_invocation_id) gid: vec3u,
    @builtin(num_workgroups) workgroups: vec3u,
) {
    let index = thread_index(gid, workgroups);
    if index >= simulation.spawn_count {
        return;
    }
    let spawn = spawns[index];
    var particle = spawn.particle;
    advance(&particle, spawn.first_step);
    particles[spawn.slot] = particle;
}

// The size and color modules and the world position of one slot, with its
// sort key: farthest first, dead and padding slots last.
@compute @workgroup_size(64)
fn evaluate_main(
    @builtin(global_invocation_id) gid: vec3u,
    @builtin(num_workgroups) workgroups: vec3u,
) {
    let index = thread_index(gid, workgroups);
    // Threads cover the sort buffer: [0, slot_count) are particle slots, and
    // [slot_count, sort_count) pads the buffer to the power of two bitonic sort needs.
    if index >= view.sort_count {
        return;
    }
    // `||` short-circuits in WGSL, so padding never reads past the particle buffer.
    let is_padding = index >= view.slot_count;
    if is_padding || !is_alive(source_particles[index]) {
        keys[index] = SortKey(DEAD_KEY, index); // dead and padding keys sort last
        // `gather_main` copies all `slot_count` sorted entries, dead slots
        // included (only the first live-count instances are drawn), so a dead
        // slot's entry is cleared rather than left from an earlier frame.
        // Padding has no `evaluated` entry: the buffer holds `slot_count`.
        if !is_padding {
            evaluated[index] = Instance(vec4f(0.0), vec4f(0.0));
        }
        return;
    }
    let particle = source_particles[index];
    let t = clamp(particle.age / max(particle.lifetime, MIN_PARTICLE_LIFETIME), 0.0, 1.0);
    // The particle's offset shifts each key of the size curve, clamped at zero.
    var size_segment = curve_segment(view.size_curve, t);
    size_segment.start = max(size_segment.start + particle.size_offset, 0.0);
    size_segment.end = max(size_segment.end + particle.size_offset, 0.0);
    let size = curve_segment_value(size_segment);
    var position = particle.position;
    if view.local_space != 0u {
        position = (view.emitter * vec4f(position, 1.0)).xyz;
    }
    let color = gradient_evaluate(view.color_gradient, t);
    let offset = position - view.camera_position;
    keys[index] = SortKey(-dot(offset, offset), index);
    evaluated[index] = Instance(vec4f(position, size), color);
}

fn key_greater(a: SortKey, b: SortKey) -> bool {
    return a.key > b.key || (a.key == b.key && a.index > b.index);
}

// One compare-and-swap pass of an ascending bitonic sort over `sort_count`
// keys, a power of two.
@compute @workgroup_size(64)
fn sort_main(
    @builtin(global_invocation_id) gid: vec3u,
    @builtin(num_workgroups) workgroups: vec3u,
) {
    let index = thread_index(gid, workgroups);
    // Sorts the whole padded buffer, [0, sort_count): padding keys take part
    // in the network and, being dead, end up last.
    if index >= view.sort_count {
        return;
    }
    let partner = index ^ sort.j;
    if partner <= index {
        return;
    }
    let a = keys[index];
    let b = keys[partner];
    let ascending = (index & sort.k) == 0u;
    if key_greater(a, b) == ascending {
        keys[index] = b;
        keys[partner] = a;
    }
}

// Copies the evaluated slots into the instance buffer in sorted order.
@compute @workgroup_size(64)
fn gather_main(
    @builtin(global_invocation_id) gid: vec3u,
    @builtin(num_workgroups) workgroups: vec3u,
) {
    let index = thread_index(gid, workgroups);
    // Only [0, slot_count) of the sorted keys name particle slots: padding
    // keys sort after every slot's, dead ones included, so they never land
    // below `slot_count`.
    if index >= view.slot_count {
        return;
    }
    instances[index] = evaluated[keys[index].index];
}
