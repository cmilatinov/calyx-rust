struct CameraUniforms {
    projection: mat4x4f,
    view: mat4x4f,
    inverse_projection: mat4x4f,
    inverse_view: mat4x4f,
    near_plane: f32,
    far_plane: f32,
    viewport_size: vec2f,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;
