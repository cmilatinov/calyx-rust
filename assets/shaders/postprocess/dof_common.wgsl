// Shared by the depth of field shaders.

struct DofUniform {
    // The camera's clip planes, to turn depth into view distance.
    near_plane: f32,
    far_plane: f32,
    // View distance that is sharp.
    focus_distance: f32,
    // Distance from the focus plane at which the blur reaches `max_blur`.
    focus_range: f32,
    // Largest circle of confusion radius, in pixels.
    max_blur: f32,
    // Spacing of the gather's spiral, in pixels at a radius of one pixel.
    radius_scale: f32,
    _padding: vec2f,
};

// View distance of a depth-buffer value. The projection is `perspective_lh`
// with OpenGL's [-1, 1] depth range, of which the depth buffer keeps [0, 1].
fn linear_depth(params: DofUniform, depth: f32) -> f32 {
    let near = params.near_plane;
    let far = params.far_plane;
    return 2.0 * near * far / (far + near - depth * (far - near));
}

// Signed circle of confusion radius in pixels: negative in front of the focus
// plane, positive behind it, growing linearly to `max_blur` at `focus_range`.
fn circle_of_confusion(params: DofUniform, depth: f32) -> f32 {
    let distance = linear_depth(params, depth);
    let t = (distance - params.focus_distance) / max(params.focus_range, 1e-4);
    return clamp(t, -1.0, 1.0) * params.max_blur;
}
