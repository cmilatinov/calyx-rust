use super::Distance;
use approx::assert_abs_diff_eq;
use nalgebra_glm::{vec2, vec3, vec4};

#[test]
fn f32_distance() {
    assert_abs_diff_eq!(3.0f32.distance(&7.0), 4.0);
    assert_abs_diff_eq!(7.0f32.distance(&3.0), 4.0);
    assert_abs_diff_eq!(0.0f32.distance(&0.0), 0.0);
}

#[test]
fn array_distance() {
    assert_abs_diff_eq!([3.0f32, 4.0].distance(&[0.0, 0.0]), 5.0);
    assert_abs_diff_eq!([0.0f32; 3].distance(&[0.0; 3]), 0.0);
}

#[test]
fn vec2_distance() {
    assert_abs_diff_eq!(vec2(0.0f32, 0.0).distance(&vec2(3.0, 4.0)), 5.0);
}

#[test]
fn vec3_distance() {
    assert_abs_diff_eq!(vec3(1.0f32, 0.0, 0.0).distance(&vec3(4.0, 0.0, 0.0)), 3.0);
}

#[test]
fn vec4_distance() {
    assert_abs_diff_eq!(
        vec4(0.0f32, 0.0, 0.0, 0.0).distance(&vec4(1.0, 0.0, 0.0, 0.0)),
        1.0
    );
}
