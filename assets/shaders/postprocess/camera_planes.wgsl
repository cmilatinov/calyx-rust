// The clip planes and fields of view of the camera a frame was drawn with, for passes that turn
// depth back into distance or position. It matches `CameraPlanes` in
// `crates/calyx_engine/src/render/postprocess/pass.rs` byte for byte (16 bytes), so a pass's
// uniform embeds it as a member and the Rust side writes the struct as it is.
//
// The projection is `perspective_lh` with OpenGL's [-1, 1] depth range, of which the depth
// buffer keeps [0, 1]. View space is left-handed: the camera looks down +z.

struct CameraPlanes {
    // Distances to the near and far planes.
    near: f32,
    far: f32,
    // Tangents of half the horizontal and vertical fields of view.
    tan_half_fov: vec2f,
};

// View depth (distance along +z) of a depth-buffer value, as `CameraPlanes::linear_depth`.
fn camera_linear_depth(planes: CameraPlanes, depth: f32) -> f32 {
    return 2.0 * planes.near * planes.far
        / (planes.far + planes.near - depth * (planes.far - planes.near));
}

// 1 / view depth of a depth-buffer value. It is affine in the depth-buffer value, and linear in
// screen space across a plane.
fn camera_inverse_depth(planes: CameraPlanes, depth: f32) -> f32 {
    return (planes.far + planes.near - depth * (planes.far - planes.near))
        / (2.0 * planes.near * planes.far);
}

// Normalized device coordinates of `uv` ((0, 0) top left, (1, 1) bottom right).
fn camera_ndc(uv: vec2f) -> vec2f {
    return vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
}

// The uv ((0, 0) top left) of normalized device coordinates `ndc`.
fn camera_uv(ndc: vec2f) -> vec2f {
    return vec2f(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// The view-space direction through `ndc`, with z = 1.
fn camera_ray(planes: CameraPlanes, ndc: vec2f) -> vec3f {
    return vec3f(ndc * planes.tan_half_fov, 1.0);
}

// The view-space position at `ndc` and view depth `z`.
fn camera_view_position(planes: CameraPlanes, ndc: vec2f, z: f32) -> vec3f {
    return camera_ray(planes, ndc) * z;
}

// The normalized device coordinates a view-space position projects to.
fn camera_project(planes: CameraPlanes, position: vec3f) -> vec2f {
    return position.xy / (planes.tan_half_fov * position.z);
}

// Distance from the camera, along its ray, to the point at `ndc` and view depth `z`.
fn camera_ray_distance(planes: CameraPlanes, ndc: vec2f, z: f32) -> f32 {
    return z * length(camera_ray(planes, ndc));
}
