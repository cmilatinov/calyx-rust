use super::Transform;
use approx::assert_abs_diff_eq;
use nalgebra::UnitQuaternion;
use nalgebra_glm as glm;
use nalgebra_glm::Vec3;

#[test]
fn basic_transform_translation() {
    let transform = Transform::default();

    assert_eq!(transform.forward(), glm::vec3(0f32, 0f32, 1f32));
    assert_eq!(
        transform.forward().scale(-1f32),
        glm::vec3(0f32, 0f32, -1f32)
    );

    assert_eq!(transform.right(), glm::vec3(1f32, 0f32, 0f32));
    assert_eq!(transform.right().scale(-1f32), glm::vec3(-1f32, 0f32, 0f32));

    assert_eq!(transform.up(), glm::vec3(0f32, 1f32, 0f32));
    assert_eq!(transform.up().scale(-1f32), glm::vec3(0f32, -1f32, 0f32));
}

#[test]
fn matrix_round_trip() {
    let t = Transform::from_components(
        Vec3::new(1.0, 2.0, 3.0),
        UnitQuaternion::from_euler_angles(0.1, 0.2, 0.3),
        Vec3::new(1.0, 1.0, 1.0),
    );
    let rt: Transform = t.matrix().into();
    assert_abs_diff_eq!(t.position, rt.position, epsilon = 1e-5);
    assert_abs_diff_eq!(t.scale, rt.scale, epsilon = 1e-5);
    let dot = t.rotation.quaternion().dot(rt.rotation.quaternion()).abs();
    assert_abs_diff_eq!(dot, 1.0, epsilon = 1e-5);
}

#[test]
fn inverse_matrix_is_actual_inverse() {
    let t = Transform::from_components(
        Vec3::new(3.0, -1.0, 2.0),
        UnitQuaternion::from_euler_angles(0.5, -0.3, 0.1),
        Vec3::new(2.0, 3.0, 4.0),
    );
    let identity = t.matrix() * t.inverse_matrix();
    for i in 0..4 {
        for j in 0..4 {
            let expected = if i == j { 1.0 } else { 0.0 };
            assert_abs_diff_eq!(identity[(i, j)], expected, epsilon = 1e-5);
        }
    }
}

#[test]
fn nlerp_single_transform_is_identity() {
    let t = Transform::from_xyz(1.0, 2.0, 3.0);
    let result = Transform::nlerp([(1.0, t)].into_iter());
    assert_abs_diff_eq!(result.position, t.position, epsilon = 1e-5);
    assert_abs_diff_eq!(result.scale, t.scale, epsilon = 1e-5);
}

#[test]
fn nlerp_equal_weights_midpoint() {
    let a = Transform::from_xyz(0.0, 0.0, 0.0);
    let b = Transform::from_xyz(2.0, 0.0, 0.0);
    let result = Transform::nlerp([(0.5, a), (0.5, b)].into_iter());
    assert_abs_diff_eq!(result.position, Vec3::new(1.0, 0.0, 0.0), epsilon = 1e-5);
}

#[test]
fn look_at_points_toward_target() {
    let mut t = Transform::from_xyz(0.0, 0.0, 0.0);
    t.look_at(&Vec3::new(0.0, 0.0, 5.0));
    let fwd = t.forward();
    assert_abs_diff_eq!(fwd, Vec3::new(0.0, 0.0, 1.0), epsilon = 1e-5);
}
