use glm::{Mat4, Vec3, vec3};

mod transform;
pub use transform::*;

pub fn compose_transform(translation: &Vec3, rotation: &Vec3, scale: &Vec3) -> Mat4 {
    let mut matrix = glm::identity();
    matrix = glm::translate(&matrix, translation);
    matrix = glm::rotate_z(&matrix, rotation.z);
    matrix = glm::rotate_y(&matrix, rotation.y);
    matrix = glm::rotate_x(&matrix, rotation.x);
    matrix = glm::scale(&matrix, scale);
    matrix
}

pub fn decompose_transform(matrix: &Mat4, translation: &mut Vec3, rotation: &mut Vec3, scale: &mut Vec3) {
    *translation = vec3(matrix.m14, matrix.m24, matrix.m34);
    let sx = glm::length(&vec3(matrix.m11, matrix.m21, matrix.m31));
    let sy = glm::length(&vec3(matrix.m12, matrix.m22, matrix.m32));
    let sz = glm::length(&vec3(matrix.m13, matrix.m23, matrix.m33));
    *scale = vec3(sx, sy, sz);
    let rotation_matrix = glm::mat4_to_mat3(matrix);
    *rotation = glm::quat_euler_angles(&glm::mat3_to_quat(&rotation_matrix)).zyx();
}

pub fn to_fov_x(aspect: f32, fov_y: f32) -> f32 {
    2.0 * ((fov_y * 0.5).tan() * aspect).atan()
}

pub fn to_fov_y(aspect: f32, fov_x: f32) -> f32 {
    2.0 * ((fov_x * 0.5).tan() / aspect).atan()
}