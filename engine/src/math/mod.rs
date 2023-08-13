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
    let rotation_matrix = &glm::mat4_to_mat3(matrix);

    // Position
    *translation = vec3(matrix[(3, 0)], matrix[(3, 1)], matrix[(3, 2)]);

    // Scale
    let column0 = vec3(rotation_matrix[(0, 0)], rotation_matrix[(1, 0)], rotation_matrix[(2, 0)]);
    let column1 = vec3(rotation_matrix[(0, 1)], rotation_matrix[(1, 1)], rotation_matrix[(2, 1)]);
    let column2 = vec3(rotation_matrix[(0, 2)], rotation_matrix[(1, 2)], rotation_matrix[(2, 2)]);
    let sx = glm::length(&column0);
    let sy = glm::length(&column1);
    let sz = glm::length(&column2);
    *scale = vec3(sx, sy, sz);

    let rotation_matrix = glm::mat3(
        column0.x / sx, column0.y / sx, column0.z / sx,
        column1.x / sy, column1.y / sy, column1.z / sy,
        column2.x / sz, column2.y / sz, column2.z / sz
    );

    let rotation_mat4 = &glm::mat3_to_mat4(&rotation_matrix);
    *rotation = glm::quat_euler_angles(&glm::to_quat(&rotation_mat4));
}
