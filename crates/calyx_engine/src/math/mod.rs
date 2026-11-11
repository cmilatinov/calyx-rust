use approx::AbsDiffEq;
use mint::Vector2;
use nalgebra::{Matrix3, Rotation3, UnitQuaternion};
use nalgebra_glm as glm;
use nalgebra_glm::{vec3, Mat4, Vec3};

/// Keyframed scalar curves evaluated over time.
pub use curve::*;
/// Distance traits and helper implementations for common math types.
pub use dist::*;
/// Color gradients evaluated over time.
pub use gradient::*;
/// Height grids read from heightmap images, sampled and meshed as terrain.
pub use heightfield::*;
/// Transform type and helpers for local and world-space math.
pub use transform::*;
/// Faceted water surfaces, meshed for their vertex waves.
pub use water::*;
/// Where meshes standing in water cut its surface, and the footprints foam rings.
pub use waterline::*;

mod curve;
mod dist;
mod gradient;
mod heightfield;
mod transform;
mod water;
mod waterline;

/// Composes a transform matrix from translation, rotation, and scale.
pub fn compose_transform(translation: &Vec3, rotation: &UnitQuaternion<f32>, scale: &Vec3) -> Mat4 {
    glm::translation(translation) * glm::quat_to_mat4(rotation) * glm::scaling(scale)
}

/// Decomposes `matrix` into translation, rotation, and scale components.
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
    // The iterative fit cannot leave its starting guess when the rotation is
    // a half turn from it, so start from the direct conversion rather than
    // the identity; the fit then only absorbs any shear left in the axes.
    // With shear the axes are not orthogonal and the conversion is not a unit
    // quaternion, so it is normalised first.
    let guess = UnitQuaternion::new_normalize(
        *UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(rotation_matrix))
            .quaternion(),
    );
    *rotation =
        UnitQuaternion::from_matrix_eps(&rotation_matrix, f32::default_epsilon(), 100, guess);
}

/// Converts a vertical field of view to a horizontal field of view for
/// `aspect`.
pub fn to_fov_x(aspect: f32, fov_y: f32) -> f32 {
    2.0 * ((fov_y * 0.5).tan() * aspect).atan()
}

/// Converts a horizontal field of view to a vertical field of view for
/// `aspect`.
pub fn to_fov_y(aspect: f32, fov_x: f32) -> f32 {
    2.0 * ((fov_x * 0.5).tan() / aspect).atan()
}

/// The point a fraction `t` along the cubic Bezier curve from `start` to
/// `end` with control points `c1` and `c2`, in Bernstein form.
pub fn cubic_bezier(start: Vec3, c1: Vec3, c2: Vec3, end: Vec3, t: f32) -> Vec3 {
    let s = 1.0 - t;
    start * (s * s * s) + c1 * (3.0 * s * s * t) + c2 * (3.0 * s * t * t) + end * (t * t * t)
}

/// The cubic Bezier control points `[start, c1, c2, end]` of the uniform
/// Catmull-Rom segment from `p1` to `p2`, whose neighbours are `p0` and `p3`.
pub fn catmull_rom_to_bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3) -> [Vec3; 4] {
    [p1, p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2]
}

/// The point a fraction `u` along the uniform Catmull-Rom segment from `p1`
/// to `p2`, whose neighbours are `p0` and `p3`, evaluated through its Bezier
/// form so curves and their gizmos share one implementation.
pub fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
    let [start, c1, c2, end] = catmull_rom_to_bezier(p0, p1, p2, p3);
    cubic_bezier(start, c1, c2, end, u)
}

/// Fits a rectangle with aspect ratio `aspect` inside `available`.
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

    /// A half turn is as far from the identity as a rotation gets, so it is
    /// the case an iterative fit started from the identity cannot leave.
    #[test]
    fn decompose_transform_keeps_a_half_turn() {
        let pos = vec3(1.0f32, 2.0, 3.0);
        let scale = vec3(3.0f32, 3.0, 3.0);
        // Exact half turns about Y, X and Z, as a scene file stores them.
        for (i, j, k) in [(0.0, 1.0, 0.0), (1.0, 0.0, 0.0), (0.0, 0.0, 1.0)] {
            let rot = UnitQuaternion::from_quaternion(nalgebra::Quaternion::new(0.0, i, j, k));
            let mat = compose_transform(&pos, &rot, &scale);

            let mut out_pos = Vec3::zeros();
            let mut out_rot = UnitQuaternion::identity();
            let mut out_scale = Vec3::zeros();
            decompose_transform(&mat, &mut out_pos, &mut out_rot, &mut out_scale);

            let dot = rot.quaternion().dot(out_rot.quaternion()).abs();
            assert_abs_diff_eq!(dot, 1.0, epsilon = 1e-5);
        }
    }

    /// A rotated child under a non-uniformly scaled parent leaves shear in
    /// the world matrix, so its normalised axes are not orthogonal; the
    /// decomposed rotation must still be a unit quaternion.
    #[test]
    fn decompose_transform_keeps_a_unit_rotation_under_shear() {
        let parent = compose_transform(
            &Vec3::zeros(),
            &UnitQuaternion::identity(),
            &vec3(2.0f32, 1.0, 1.0),
        );
        let child = compose_transform(
            &Vec3::zeros(),
            &UnitQuaternion::from_euler_angles(0.0, 0.0, std::f32::consts::FRAC_PI_4),
            &vec3(1.0f32, 1.0, 1.0),
        );

        let mut out_pos = Vec3::zeros();
        let mut out_rot = UnitQuaternion::identity();
        let mut out_scale = Vec3::zeros();
        decompose_transform(
            &(parent * child),
            &mut out_pos,
            &mut out_rot,
            &mut out_scale,
        );

        assert_abs_diff_eq!(out_rot.quaternion().norm(), 1.0, epsilon = 1e-5);
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
    fn a_cubic_bezier_runs_from_start_to_end_through_its_bernstein_midpoint() {
        let (start, c1, c2, end) = (
            vec3(0.0f32, 0.0, 0.0),
            vec3(1.0f32, 4.0, 0.0),
            vec3(5.0f32, 4.0, 2.0),
            vec3(6.0f32, 0.0, 2.0),
        );
        assert_abs_diff_eq!(cubic_bezier(start, c1, c2, end, 0.0), start, epsilon = 1e-6);
        assert_abs_diff_eq!(cubic_bezier(start, c1, c2, end, 1.0), end, epsilon = 1e-6);
        let midpoint = (start + c1 * 3.0 + c2 * 3.0 + end) / 8.0;
        assert_abs_diff_eq!(
            cubic_bezier(start, c1, c2, end, 0.5),
            midpoint,
            epsilon = 1e-6
        );
    }

    /// The uniform Catmull-Rom polynomial, as the camera path evaluated it
    /// before the curve went through its Bezier form.
    fn catmull_rom_polynomial(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, u: f32) -> Vec3 {
        let u2 = u * u;
        let u3 = u2 * u;
        (p1 * 2.0
            + (p2 - p0) * u
            + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * u2
            + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * u3)
            * 0.5
    }

    #[test]
    fn catmull_rom_through_its_bezier_form_matches_the_polynomial() {
        let (p0, p1, p2, p3) = (
            vec3(-3.0f32, 1.0, 0.0),
            vec3(0.0f32, 0.0, 0.0),
            vec3(4.0f32, 2.0, 1.0),
            vec3(5.0f32, -1.0, 6.0),
        );
        for u in [0.0f32, 0.1, 0.25, 0.5, 0.7, 0.9, 1.0] {
            assert_abs_diff_eq!(
                catmull_rom(p0, p1, p2, p3, u),
                catmull_rom_polynomial(p0, p1, p2, p3, u),
                epsilon = 1e-5
            );
        }
        let [start, _, _, end] = catmull_rom_to_bezier(p0, p1, p2, p3);
        assert_eq!(start, p1);
        assert_eq!(end, p2);
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
