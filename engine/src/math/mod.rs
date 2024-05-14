use glm::{vec3, Mat4, Vec3};
use mint::Vector2;
use nalgebra::UnitQuaternion;

use russimp::Matrix4x4;
pub use transform::*;

mod transform;

pub fn compose_transform(translation: &Vec3, rotation: &Vec3, scale: &Vec3) -> Mat4 {
    let mut matrix = glm::identity();
    matrix = glm::translate(&matrix, translation);
    matrix = glm::rotate_z(&matrix, rotation.z);
    matrix = glm::rotate_y(&matrix, rotation.y);
    matrix = glm::rotate_x(&matrix, rotation.x);
    matrix = glm::scale(&matrix, scale);
    matrix
}

pub fn decompose_transform(
    matrix: &Mat4,
    translation: &mut Vec3,
    rotation: &mut Vec3,
    scale: &mut Vec3,
) {
    *translation = vec3(matrix.m14, matrix.m24, matrix.m34);
    let sx = glm::length(&vec3(matrix.m11, matrix.m21, matrix.m31));
    let sy = glm::length(&vec3(matrix.m12, matrix.m22, matrix.m32));
    let sz = glm::length(&vec3(matrix.m13, matrix.m23, matrix.m33));
    *scale = vec3(sx, sy, sz);
    *rotation = glm::quat_euler_angles(
        UnitQuaternion::from_matrix_eps(
            &glm::mat4_to_mat3(matrix),
            f32::EPSILON,
            1000,
            UnitQuaternion::identity(),
        )
        .quaternion(),
    );
}

pub fn to_fov_x(aspect: f32, fov_y: f32) -> f32 {
    2.0 * ((fov_y * 0.5).tan() * aspect).atan()
}

pub fn to_fov_y(aspect: f32, fov_x: f32) -> f32 {
    2.0 * ((fov_x * 0.5).tan() / aspect).atan()
}

pub fn fit_aspect(aspect: f32, available: impl Into<Vector2<f32>>) -> Vector2<f32> {
    let available: Vector2<f32> = available.into();
    let available_aspect = available.x / available.y;
    if available_aspect > aspect {
        Vector2 {
            x: available.y * aspect,
            y: available.y,
        }
    } else {
        Vector2 {
            x: available.x,
            y: available.x / aspect,
        }
    }
}

pub fn mat4_from_russimp(matrix: &Matrix4x4) -> Mat4 {
    let mut result = Mat4::default();
    result.copy_from_slice(&[
        matrix.a1, matrix.b1, matrix.c1, matrix.d1, matrix.a2, matrix.b2, matrix.c2, matrix.d2,
        matrix.a3, matrix.b3, matrix.c3, matrix.d3, matrix.a4, matrix.b4, matrix.c4, matrix.d4,
    ]);
    result
}
