// The blended ground material's uniform: `@group(3) @binding(2)` of `shaders/mesh/ground.wgsl`,
// and `@group(2) @binding(2)` of its geometry pass, `shaders/mesh/ground_gbuffer.wgsl`, which
// binds the same bind group. Colours are linear; each layer colour's alpha is how strongly the
// layer's detail texture shades it (0 draws the flat colour).
struct GroundProperties {
    // The base layer, wherever the masks leave room.
    grass_color: vec4f,
    // Mask A, R.
    grass_dark_color: vec4f,
    // Mask A, G.
    grass_light_color: vec4f,
    // Mask A, B.
    dirt_color: vec4f,
    // Mask A, A: the sand lanes.
    sand_color: vec4f,
    // Mask B, R.
    gravel_color: vec4f,
    // Mask B, G: a tilled field's soil, between its crop rows. Fields keep crisp edges: they
    // are read from the unwarped mask, and mask B's alpha says whether their rows run along X.
    field_color: vec4f,
    // The crop rows of a field, one metre apart.
    crop_color: vec4f,
    // Mask B, B: the wet earth along a creek.
    mud_color: vec4f,
    // x: metres one detail tile spans; y: how much larger the second, turned detail sample is;
    // z: metres across the large brightness noise; w: how much that noise shades the ground.
    detail: vec4f,
    // x: metres the masks are warped by noise, so their edges are not straight; y: the height
    // band over which layers blend, in detail units (small is crisp); z: the share of each metre
    // a crop row covers; w: the mud's roughness.
    blend: vec4f,
    // xy: the size in metres the masks span (the terrain's size); z: 1 lights the ground as level
    // ground whatever its slope (a flat ground whose only slopes are hidden, such as a creek's
    // carved channel under its banks), 0 by its own normal; w: unused.
    mask: vec4f,
    // The world rectangle the marks texture covers: x, z of its lower corner, then width and
    // depth. A zero size draws no marks.
    marks_rect: vec4f,
    roughness: f32,
    // As the PBR material's `outline_style`.
    outline_style: u32,
};
