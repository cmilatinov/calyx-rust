use super::*;
use nalgebra_glm::{vec2, vec3};

fn square_camera() -> Camera {
    // 90 degree horizontal and vertical field of view, so the screen corners sit at x = +-1, y = +-1.
    Camera::new(1.0, 90f32.to_radians(), 0.1, 1000.0)
}

#[test]
fn screen_centre_ray_points_down_camera_forward() {
    let camera = square_camera();
    let transform = Transform::from_xyz(0.0, 0.0, 0.0);

    let ray = camera
        .screen_ray(&transform, vec2(100.0, 100.0), vec2(200.0, 200.0))
        .expect("centre ray should exist");

    assert!(ray.direction.x.abs() < 1e-5);
    assert!(ray.direction.y.abs() < 1e-5);
    assert!((ray.direction.z - 1.0).abs() < 1e-5);
}

#[test]
fn top_left_corner_ray_points_to_upper_left_of_view() {
    let camera = square_camera();
    let transform = Transform::from_xyz(0.0, 0.0, 0.0);

    let ray = camera
        .screen_ray(&transform, vec2(0.0, 0.0), vec2(200.0, 200.0))
        .expect("corner ray should exist");

    let expected = vec3(-1.0, 1.0, 1.0).normalize();
    assert!((ray.direction - expected).norm() < 1e-5);
    assert!(ray.origin.norm() < 1e-5);
}

#[test]
fn empty_viewport_has_no_ray() {
    let camera = square_camera();
    let transform = Transform::from_xyz(0.0, 0.0, 0.0);

    assert!(camera
        .screen_ray(&transform, vec2(0.0, 0.0), vec2(0.0, 200.0))
        .is_none());
}
