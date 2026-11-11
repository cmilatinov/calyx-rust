// The water material's uniform: `@group(3) @binding(0)` of `shaders/mesh/water.wgsl`, and
// `@group(2) @binding(0)` of its geometry pass, `shaders/mesh/water_gbuffer.wgsl`, which binds
// the same bind group. The renderer fills it from each Water component every frame (see
// `render/water.rs`); colours are linear.
struct WaterProperties {
    // The colour `shore_distance` or more from the shore.
    deep_color: vec4f,
    // The colour at the shore.
    shallow_color: vec4f,
    // The colour of the foam.
    foam_color: vec4f,
    // The scene's brightest directional light's colour times the glint strength, as HDR
    // emission. Black draws no glints.
    glint_color: vec4f,
    // World direction that light travels along, in xyz.
    light_direction: vec4f,
    // Height of the wave crests above the rest surface, in local units.
    wave_amplitude: f32,
    // Distance between crests of the main wave, in local units.
    wave_length: f32,
    // How fast the water flows downstream, in local units per second.
    flow_speed: f32,
    // The water's clock, in seconds.
    time: f32,
    // Distance from the shore over which the colour runs from shallow to deep.
    shore_distance: f32,
    // Width of the foam band along the shore.
    foam_width: f32,
    // How much of the open water drifting foam patches cover, from 0 to 1.
    foam_patches: f32,
    // Width of the foam band around what stands in the water.
    obstacle_foam: f32,
    // Distance from what stands in the water over which the colour runs from shallow to deep.
    obstacle_shallows: f32,
    // How much the brightness of neighbouring facets differs.
    facet_variation: f32,
    roughness: f32,
    // Which ink outline the Outline effect draws around the water, as a PBR material's.
    outline_style: u32,
    // 0 shades each facet flat; 1 shades the waves smooth, with normals from their slope.
    shading: u32,
};
