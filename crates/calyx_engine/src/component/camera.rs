use crate as calyx_engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::render::{Camera, GizmoIcon, Gizmos};
use crate::scene::{GameObject, Scene};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use egui::Color32;
use nalgebra_glm::Vec4;
use serde::{Deserialize, Serialize};

/// Scene component that marks a game object as a render camera. A disabled
/// camera is never selected as the scene's main camera.
#[derive(TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "a85867d2-3e68-42b2-b943-ea78c7c6ddb5"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Camera")]
#[serde(default)]
pub struct ComponentCamera {
    /// Horizontal field of view in radians: the angle between the left and
    /// right edges of the view. The vertical angle follows from the aspect
    /// of the view the camera renders into. Views build their projection
    /// from it through [`ComponentCamera::camera`].
    #[reflect_attr(angle, min = 30.0, max = 160.0, speed = 0.1)]
    pub fov: f32,
    /// Near clipping plane distance.
    #[reflect_attr(min = 0.01, speed = 0.01)]
    pub near_plane: f32,
    /// Far clipping plane distance.
    #[reflect_attr(min = 20.0, max = 1000.0, speed = 1.0)]
    pub far_plane: f32,
    /// Clear color used when this camera renders.
    pub clear_color: Color32,
}

impl Default for ComponentCamera {
    fn default() -> Self {
        Self {
            fov: 70.0f32.to_radians(),
            near_plane: 0.1,
            far_plane: 100.0,
            clear_color: Color32::BLACK,
        }
    }
}

impl ComponentCamera {
    /// The render camera this component projects through at `aspect`
    /// (width over height): `fov` stays the horizontal angle at every
    /// aspect, so a taller view shows more above and below. Every view of a
    /// scene camera, the game view, thumbnails and the camera gizmo, builds
    /// its projection here.
    pub fn camera(&self, aspect: f32) -> Camera {
        Camera::new(aspect, self.fov, self.near_plane, self.far_plane)
    }
}

impl Component for ComponentCamera {
    /// Main camera selection skips a disabled camera.
    fn can_disable(&self) -> bool {
        true
    }

    fn draw_gizmos(&self, scene: &Scene, game_object: GameObject, gizmos: &mut Gizmos) {
        let transform = scene.world_transform(game_object);
        gizmos.set_color(&Vec4::new(1.0, 1.0, 1.0, 1.0));
        gizmos.icon(GizmoIcon::Camera, &transform.position, 1.0);
        let camera = self.camera(gizmos.game_view_aspect());
        gizmos.wire_frustum(&transform, &camera);
    }
}

#[cfg(test)]
mod tests {
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
            game_view_aspect: gizmo_options.game_view_aspect,
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
            },
        );
        assert_abs_diff_eq!(right, half_width, epsilon = 1e-3);
        assert_abs_diff_eq!(up, half_width * 3.0 / 4.0, epsilon = 1e-3);

        // Without a game view aspect, the gizmo falls back to 16:9.
        let (right, up) = gizmo_far_plane_half_extents(&camera, GizmoOptions::default());
        assert_abs_diff_eq!(right, half_width, epsilon = 1e-3);
        assert_abs_diff_eq!(up, half_width * 9.0 / 16.0, epsilon = 1e-3);
    }
}
