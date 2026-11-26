use super::*;
use crate::assets::mesh::Mesh;
use crate::test_support::TestAssets;
use nalgebra_glm::vec3;

#[test]
fn icon_size_is_clamped_to_pixel_range() {
    let assets = TestAssets::engine();
    let asset_context = assets.asset_context();
    let context = asset_context.lock_read();
    let camera = Camera::default();
    let camera_transform = Transform::default();
    let position = vec3(0.0, 0.0, 100.0);
    let mut circle_list = Vec::new();
    let mut cube_list = Vec::new();
    let mut lines_mesh = Mesh::new(&context.render_context);
    let mut points_mesh = Mesh::new(&context.render_context);
    let mut icons_mesh = Mesh::new(&context.render_context);
    let gizmos = Gizmos {
        camera: &camera,
        camera_transform: &camera_transform,
        viewport_size: [1920.0, 1080.0],
        options: Default::default(),
        object_id: 0,
        color: vec4(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circle_list,
        cube_list: &mut cube_list,
        lines_mesh: &mut lines_mesh,
        points_mesh: &mut points_mesh,
        icons_mesh: &mut icons_mesh,
    };

    let size = gizmos.clamped_icon_world_size(&position, 0.01);
    let projected_pixels = size * camera.projection[(1, 1)].abs() * 1080.0 / (2.0 * position.z);

    assert!((projected_pixels - MIN_ICON_SIZE_PIXELS).abs() < 0.001);

    let size = gizmos.clamped_icon_world_size(&vec3(0.0, 0.0, 1.0), 10.0);
    let projected_pixels = size * camera.projection[(1, 1)].abs() * 1080.0 / 2.0;

    assert!((projected_pixels - MAX_ICON_SIZE_PIXELS).abs() < 0.001);
}

#[test]
fn a_cubic_bezier_gizmo_adds_its_segments_from_start_to_end() {
    let assets = TestAssets::engine();
    let asset_context = assets.asset_context();
    let context = asset_context.lock_read();
    let camera = Camera::default();
    let camera_transform = Transform::default();
    let mut circle_list = Vec::new();
    let mut cube_list = Vec::new();
    let mut lines_mesh = Mesh::new(&context.render_context);
    let mut points_mesh = Mesh::new(&context.render_context);
    let mut icons_mesh = Mesh::new(&context.render_context);
    let mut gizmos = Gizmos {
        camera: &camera,
        camera_transform: &camera_transform,
        viewport_size: [1920.0, 1080.0],
        options: Default::default(),
        object_id: 0,
        color: vec4(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circle_list,
        cube_list: &mut cube_list,
        lines_mesh: &mut lines_mesh,
        points_mesh: &mut points_mesh,
        icons_mesh: &mut icons_mesh,
    };
    let (start, c1, c2, end) = (
        vec3(0.0, 0.0, 0.0),
        vec3(1.0, 3.0, 0.0),
        vec3(4.0, 3.0, 0.0),
        vec3(5.0, 0.0, 0.0),
    );

    gizmos.cubic_bezier(&start, &c1, &c2, &end, 7);

    let vertices = &lines_mesh.vertices;
    assert_eq!(vertices.len(), 14, "7 lines of 2 vertices");
    assert_eq!(vertices[0], start);
    assert!((vertices[13] - end).norm() < 1e-6);
    // The pieces join up into one polyline.
    for line in 1..7 {
        assert_eq!(vertices[2 * line], vertices[2 * line - 1]);
    }
}

#[test]
fn icon_mesh_stores_object_id_for_picking() {
    let assets = TestAssets::engine();
    let asset_context = assets.asset_context();
    let context = asset_context.lock_read();
    let camera = Camera::default();
    let camera_transform = Transform::default();
    let mut circle_list = Vec::new();
    let mut cube_list = Vec::new();
    let mut lines_mesh = Mesh::new(&context.render_context);
    let mut points_mesh = Mesh::new(&context.render_context);
    let mut icons_mesh = Mesh::new(&context.render_context);
    let mut gizmos = Gizmos {
        camera: &camera,
        camera_transform: &camera_transform,
        viewport_size: [1920.0, 1080.0],
        options: Default::default(),
        object_id: 17,
        color: vec4(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circle_list,
        cube_list: &mut cube_list,
        lines_mesh: &mut lines_mesh,
        points_mesh: &mut points_mesh,
        icons_mesh: &mut icons_mesh,
    };

    gizmos.icon(GizmoIcon::Camera, &vec3(0.0, 0.0, 10.0), 1.0);

    assert_eq!(gizmos.icons_mesh.uvs[3].len(), 4);
    assert!(gizmos.icons_mesh.uvs[3].iter().all(|uv| uv.x == 17.0));
}
#[test]
fn wire_cone_places_its_base_circle_and_edges_at_the_slant_length() {
    let assets = TestAssets::engine();
    let asset_context = assets.asset_context();
    let context = asset_context.lock_read();
    let camera = Camera::default();
    let camera_transform = Transform::default();
    let mut circle_list = Vec::new();
    let mut cube_list = Vec::new();
    let mut lines_mesh = Mesh::new(&context.render_context);
    let mut points_mesh = Mesh::new(&context.render_context);
    let mut icons_mesh = Mesh::new(&context.render_context);
    let mut gizmos = Gizmos {
        camera: &camera,
        camera_transform: &camera_transform,
        viewport_size: [1920.0, 1080.0],
        options: Default::default(),
        object_id: 0,
        color: vec4(1.0, 1.0, 1.0, 1.0),
        opacity: 1.0,
        depth_test_enabled: true,
        circle_list: &mut circle_list,
        cube_list: &mut cube_list,
        lines_mesh: &mut lines_mesh,
        points_mesh: &mut points_mesh,
        icons_mesh: &mut icons_mesh,
    };
    let apex = vec3(1.0, 2.0, 3.0);

    gizmos.wire_cone(&apex, &vec3(0.0, -2.0, 0.0), 30f32.to_radians(), 4.0);

    assert_eq!(gizmos.circle_list.len(), 1);
    let circle: Mat4 = gizmos.circle_list[0].transform.into();
    let center = circle.column(3).xyz();
    let expected_center = apex + vec3(0.0, -4.0 * 30f32.to_radians().cos(), 0.0);
    assert!((center - expected_center).norm() < 1e-5, "{center}");
    let normal = circle.column(2).xyz();
    assert!(
        (normal.norm() - 2.0).abs() < 1e-5,
        "the circle radius is 4 sin 30"
    );
    assert!((glm::normalize(&normal) - vec3(0.0, -1.0, 0.0)).norm() < 1e-5);
    assert_eq!(gizmos.lines_mesh.vertices.len(), 8);
    for edge in gizmos.lines_mesh.vertices.chunks(2) {
        assert_eq!(edge[0], apex);
        assert!(((edge[1] - apex).norm() - 4.0).abs() < 1e-4);
    }

    gizmos.wire_cone(&apex, &vec3(0.0, 0.0, 1.0), 0.0, 4.0);
    assert_eq!(
        gizmos.circle_list.len(),
        1,
        "a zero angle cone has no base circle"
    );
    assert_eq!(gizmos.lines_mesh.vertices.len(), 16);
}
