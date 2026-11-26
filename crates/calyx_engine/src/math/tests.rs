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
