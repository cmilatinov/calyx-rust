use approx::AbsDiffEq;
use mint::Vector2;
use nalgebra::{Matrix3, UnitQuaternion};
use nalgebra_glm as glm;
use nalgebra_glm::{vec3, Mat4, Vec3};

pub use dist::*;
use russimp_ng::Matrix4x4;
pub use transform::*;

mod dist;
mod transform;

pub fn compose_transform(translation: &Vec3, rotation: &UnitQuaternion<f32>, scale: &Vec3) -> Mat4 {
    glm::translation(translation) * glm::quat_to_mat4(rotation) * glm::scaling(scale)
}

pub fn decompose_transform(
    matrix: &Mat4,
    translation: &mut Vec3,
    rotation: &mut UnitQuaternion<f32>,
    scale: &mut Vec3,
) {
    *translation = vec3(matrix.m14, matrix.m24, matrix.m34);
    let mut x_axis = vec3(matrix.m11, matrix.m21, matrix.m31);
    let mut y_axis = vec3(matrix.m12, matrix.m22, matrix.m32);
    let mut z_axis = vec3(matrix.m13, matrix.m23, matrix.m33);
    let mut sx = glm::length(&x_axis);
    let sy = glm::length(&y_axis);
    let sz = glm::length(&z_axis);
    if sx > 0.0 {
        x_axis /= sx;
    }
    if sy > 0.0 {
        y_axis /= sy;
    }
    if sz > 0.0 {
        z_axis /= sz;
    }
    if Matrix3::from_columns(&[x_axis, y_axis, z_axis]).determinant() < 0.0 {
        sx = -sx;
        x_axis = -x_axis;
    }
    *scale = vec3(sx, sy, sz);
    let rotation_matrix = Matrix3::from_columns(&[x_axis, y_axis, z_axis]);
    *rotation = UnitQuaternion::from_matrix_eps(
        &rotation_matrix,
        f32::default_epsilon(),
        100,
        UnitQuaternion::identity(),
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
    Mat4::from_row_slice(&[
        matrix.a1, matrix.a2, matrix.a3, matrix.a4, matrix.b1, matrix.b2, matrix.b3, matrix.b4,
        matrix.c1, matrix.c2, matrix.c3, matrix.c4, matrix.d1, matrix.d2, matrix.d3, matrix.d4,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    use nalgebra::UnitQuaternion;
    use nalgebra_glm::{vec3, Vec3};

    #[test]
    fn compose_decompose_round_trip() {
        let pos = vec3(1.0f32, 2.0, 3.0);
        let rot = UnitQuaternion::from_euler_angles(0.1, 0.2, 0.3);
        let scale = vec3(2.0f32, 3.0, 4.0);

        let mat = compose_transform(&pos, &rot, &scale);

        let mut out_pos = Vec3::zeros();
        let mut out_rot = UnitQuaternion::identity();
        let mut out_scale = Vec3::zeros();
        decompose_transform(&mat, &mut out_pos, &mut out_rot, &mut out_scale);

        assert_abs_diff_eq!(out_pos, pos, epsilon = 1e-5);
        assert_abs_diff_eq!(out_scale, scale, epsilon = 1e-5);
        let dot = rot.quaternion().dot(out_rot.quaternion()).abs();
        assert_abs_diff_eq!(dot, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn decompose_transform_preserves_negative_scale() {
        let pos = vec3(1.0f32, 2.0, 3.0);
        let rot = UnitQuaternion::from_euler_angles(0.1, 0.2, 0.3);
        let scale = vec3(-2.0f32, 3.0, 4.0);

        let mat = compose_transform(&pos, &rot, &scale);

        let mut out_pos = Vec3::zeros();
        let mut out_rot = UnitQuaternion::identity();
        let mut out_scale = Vec3::zeros();
        decompose_transform(&mat, &mut out_pos, &mut out_rot, &mut out_scale);

        assert_abs_diff_eq!(out_pos, pos, epsilon = 1e-5);
        assert_abs_diff_eq!(out_scale, scale, epsilon = 1e-5);
        let dot = rot.quaternion().dot(out_rot.quaternion()).abs();
        assert_abs_diff_eq!(dot, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn fov_x_y_round_trip() {
        let aspect = 16.0f32 / 9.0;
        let fov_y = std::f32::consts::FRAC_PI_4;
        let fov_x = to_fov_x(aspect, fov_y);
        let recovered = to_fov_y(aspect, fov_x);
        assert_abs_diff_eq!(recovered, fov_y, epsilon = 1e-5);
    }

    #[test]
    fn fit_aspect_wide_available() {
        let result = fit_aspect(1.0, [4.0f32, 2.0f32]);
        assert_abs_diff_eq!(result.x, 2.0, epsilon = 1e-5);
        assert_abs_diff_eq!(result.y, 2.0, epsilon = 1e-5);
    }

    #[test]
    fn fit_aspect_tall_available() {
        let result = fit_aspect(2.0, [2.0f32, 4.0f32]);
        assert_abs_diff_eq!(result.x, 2.0, epsilon = 1e-5);
        assert_abs_diff_eq!(result.y, 1.0, epsilon = 1e-5);
    }
}
