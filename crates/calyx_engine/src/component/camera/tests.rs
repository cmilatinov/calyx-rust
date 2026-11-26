use super::*;
use crate::assets::mesh::Mesh;
use crate::math::Transform;
use crate::render::GizmoOptions;
use crate::test_support::{test_scene, TestAssets};
use approx::assert_abs_diff_eq;
use nalgebra_glm::Vec3;

/// The horizontal and vertical angles a perspective projection shows.
fn projection_fovs(camera: &Camera) -> (f32, f32) {
    (
        2.0 * (1.0 / camera.projection[(0, 0)]).atan(),
        2.0 * (1.0 / camera.projection[(1, 1)]).atan(),
    )
}

#[test]
fn camera_projects_its_fov_as_the_horizontal_angle_at_any_aspect() {
    let component = ComponentCamera {
        fov: 90.0f32.to_radians(),
        ..Default::default()
    };

    // A 90 degree horizontal angle puts the side edges at tan(45) = 1, so
    // the top edge sits at 1 / aspect: 2 * atan(9 / 16) and 2 * atan(3 / 4).
    for (aspect, vertical_degrees) in [(16.0 / 9.0, 58.715_5), (4.0 / 3.0, 73.739_8)] {
        let camera = component.camera(aspect);
        let (horizontal, vertical) = projection_fovs(&camera);
        assert_abs_diff_eq!(horizontal, 90.0f32.to_radians(), epsilon = 1e-5);
        assert_abs_diff_eq!(vertical.to_degrees(), vertical_degrees, epsilon = 1e-3);
        assert_abs_diff_eq!(camera.aspect, aspect);
    }
}

/// The half extents, right and up, of the far plane the camera gizmo of
/// a camera at the origin draws in a view with `gizmo_options`.
fn gizmo_far_plane_half_extents(
    camera: &ComponentCamera,
    gizmo_options: GizmoOptions,
) -> (f32, f32) {
    let assets = TestAssets::engine();
    let context = assets.asset_context();
    let context = context.lock_read();
    let mut scene = test_scene();
    let game_object = scene.create(None, None);
    let view = Camera::default();
    let view_transform = Transform::default();
    let mut circle_list = Vec::new();
    let mut cube_list = Vec::new();
    let mut lines_mesh = Mesh::new(&context.render_context);
    let mut points_mesh = Mesh::new(&context.render_context);
    let mut icons_mesh = Mesh::new(&context.render_context);
    let mut gizmos = Gizmos {
        camera: &view,
        camera_transform: &view_transform,
        viewport_size: [1920.0, 1080.0],
        options: gizmo_options,
        object_id: 0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circle_list,
        cube_list: &mut cube_list,
        lines_mesh: &mut lines_mesh,
        points_mesh: &mut points_mesh,
        icons_mesh: &mut icons_mesh,
    };
    camera.draw_gizmos(&scene, game_object, &mut gizmos);

    let far: Vec<Vec3> = lines_mesh
        .vertices
        .iter()
        .filter(|vertex| (vertex.z - camera.far_plane).abs() < 1e-2)
        .copied()
        .collect();
    assert!(!far.is_empty(), "the gizmo draws its far plane");
    let right = far.iter().map(|vertex| vertex.x.abs()).fold(0.0, f32::max);
    let up = far.iter().map(|vertex| vertex.y.abs()).fold(0.0, f32::max);
    (right, up)
}

#[test]
fn camera_gizmo_frames_the_game_view_aspect_with_the_horizontal_fov() {
    let camera = ComponentCamera {
        fov: 60.0f32.to_radians(),
        far_plane: 10.0,
        ..Default::default()
    };
    // tan(30 degrees) * 10, the far plane's half width at a 60 degree horizontal angle.
    let half_width = 5.773_502_7;

    let (right, up) = gizmo_far_plane_half_extents(
        &camera,
        GizmoOptions {
            game_view_aspect: Some(4.0 / 3.0),
            ..Default::default()
        },
    );
    assert_abs_diff_eq!(right, half_width, epsilon = 1e-3);
    assert_abs_diff_eq!(up, half_width * 3.0 / 4.0, epsilon = 1e-3);

    // Without a game view aspect, the gizmo falls back to 16:9.
    let (right, up) = gizmo_far_plane_half_extents(&camera, GizmoOptions::default());
    assert_abs_diff_eq!(right, half_width, epsilon = 1e-3);
    assert_abs_diff_eq!(up, half_width * 9.0 / 16.0, epsilon = 1e-3);
}
