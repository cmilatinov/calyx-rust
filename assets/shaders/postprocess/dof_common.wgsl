//#include "shaders/postprocess/camera_planes.wgsl"

// Shared by the depth of field shaders.

struct DofUniform {
    // The camera, to turn depth into view distance.
    camera: CameraPlanes,
    // View distance that is sharp.
    focus_distance: f32,
    // Distance from the focus plane at which the blur reaches `max_blur`.
    focus_range: f32,
    // Largest circle of confusion radius, in pixels.
    max_blur: f32,
    // Spacing of the gather's spiral, in pixels at a radius of one pixel.
    radius_scale: f32,
};

// Signed circle of confusion radius in pixels: negative in front of the focus
// plane, positive behind it, growing linearly to `max_blur` at `focus_range`.
fn circle_of_confusion(params: DofUniform, depth: f32) -> f32 {
    let distance = camera_linear_depth(params.camera, depth);
    let t = (distance - params.focus_distance) / max(params.focus_range, 1e-4);
    return clamp(t, -1.0, 1.0) * params.max_blur;
}
