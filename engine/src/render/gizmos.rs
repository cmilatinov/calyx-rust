use crate::assets::mesh::Mesh;
use crate::math::{compose_transform, Transform};
use crate::render::{Camera, GizmoInstance};
use nalgebra::UnitQuaternion;
use nalgebra_glm as glm;
use nalgebra_glm::{vec2, vec3, vec4, Mat4, Vec3, Vec4};

const MIN_ICON_SIZE_PIXELS: f32 = 24.0;
const MAX_ICON_SIZE_PIXELS: f32 = 48.0;

/// Built-in camera-facing editor icon shapes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GizmoIcon {
    /// Camera object icon.
    Camera,
    /// Light object icon.
    Light,
}

/// Immediate-mode helper used by components to emit debug gizmo geometry.
pub struct Gizmos<'a> {
    pub(crate) camera: &'a Camera,
    pub(crate) camera_transform: &'a Transform,
    pub(crate) viewport_size: [f32; 2],
    pub(crate) object_id: u32,
    pub(crate) color: Vec4,
    pub(crate) opacity: f32,
    pub(crate) depth_test_enabled: bool,
    pub(crate) circle_list: &'a mut Vec<GizmoInstance>,
    pub(crate) cube_list: &'a mut Vec<GizmoInstance>,
    pub(crate) lines_mesh: &'a mut Mesh,
    pub(crate) points_mesh: &'a mut Mesh,
    pub(crate) icons_mesh: &'a mut Mesh,
}

impl Gizmos<'_> {
    /// Emits a wireframe sphere centered at `center`.
    pub fn wire_sphere(&mut self, center: &Vec3, radius: f32) {
        let translation = glm::translate(&Mat4::identity(), center);
        let scale = vec3(radius, radius, radius);
        self.circle_list
            .push(self.gizmo_instance(glm::scale(&translation, &scale), true));
        self.circle_list.push(self.gizmo_instance(
            glm::scale(&glm::rotate_x(&translation, 90.0f32.to_radians()), &scale),
            true,
        ));
        self.circle_list.push(self.gizmo_instance(
            glm::scale(&glm::rotate_y(&translation, 90.0f32.to_radians()), &scale),
            true,
        ));

        let to_camera = self.camera_transform.position - center;
        let to_camera_normal = glm::normalize(&to_camera);
        let distance = to_camera.magnitude();
        let alpha = std::f32::consts::FRAC_PI_2 - (radius / distance).asin();
        let r = radius * alpha.sin();
        let l = radius * alpha.cos();
        let t = glm::scale(
            &glm::inverse(&glm::look_at(
                &(center + l * to_camera_normal),
                &self.camera_transform.position,
                &vec3(0.0, 1.0, 0.0),
            )),
            &vec3(r, r, r),
        );
        self.circle_list.push(self.gizmo_instance(t, false));
    }

    /// Emits a wireframe cube centered at `position`.
    pub fn wire_cube(&mut self, position: &Vec3, size: &Vec3) {
        self.wire_cube_transform(compose_transform(
            position,
            &UnitQuaternion::identity(),
            size,
        ));
    }

    /// Emits a wireframe cube with an explicit transform.
    pub fn wire_cube_transform(&mut self, transform: Mat4) {
        self.cube_list.push(self.gizmo_instance(transform, false));
    }

    /// Emits a wireframe frustum from camera projection parameters.
    pub fn wire_frustum(
        &mut self,
        transform: &Transform,
        aspect: f32,
        fov: f32,
        near_plane: f32,
        far_plane: f32,
    ) {
        let camera = Camera::new(aspect, fov, near_plane, far_plane);
        let matrix = (camera.projection * transform.inverse_matrix())
            .try_inverse()
            .unwrap_or_else(Mat4::identity);
        let _n1 = matrix * Vec4::new(-1.0, -1.0, -1.0, 1.0);
        let n1 = (_n1 / _n1.w).xyz();
        let _n2 = matrix * Vec4::new(-1.0, 1.0, -1.0, 1.0);
        let n2 = (_n2 / _n2.w).xyz();
        let _n3 = matrix * Vec4::new(1.0, 1.0, -1.0, 1.0);
        let n3 = (_n3 / _n3.w).xyz();
        let _n4 = matrix * Vec4::new(1.0, -1.0, -1.0, 1.0);
        let n4 = (_n4 / _n4.w).xyz();
        let _f1 = matrix * Vec4::new(-1.0, -1.0, 1.0, 1.0);
        let f1 = (_f1 / _f1.w).xyz();
        let _f2 = matrix * Vec4::new(-1.0, 1.0, 1.0, 1.0);
        let f2 = (_f2 / _f2.w).xyz();
        let _f3 = matrix * Vec4::new(1.0, 1.0, 1.0, 1.0);
        let f3 = (_f3 / _f3.w).xyz();
        let _f4 = matrix * Vec4::new(1.0, -1.0, 1.0, 1.0);
        let f4 = (_f4 / _f4.w).xyz();
        self.line(&n1, &n2);
        self.line(&n2, &n3);
        self.line(&n3, &n4);
        self.line(&n4, &n1);
        self.line(&f1, &f2);
        self.line(&f2, &f3);
        self.line(&f3, &f4);
        self.line(&f4, &f1);
        self.line(&f1, &n1);
        self.line(&f2, &n2);
        self.line(&f3, &n3);
        self.line(&f4, &n4);
    }

    /// Emits a colored line segment.
    pub fn line(&mut self, start: &Vec3, end: &Vec3) {
        self.lines_mesh.vertices.push(*start);
        self.lines_mesh.vertices.push(*end);
        self.lines_mesh.uvs[0].extend(std::iter::repeat_n(self.color.xy(), 2));
        self.lines_mesh.uvs[1].extend(std::iter::repeat_n(vec2(self.color.z, self.alpha()), 2));
    }

    /// Emits a colored point.
    pub fn point(&mut self, point: &Vec3) {
        self.points_mesh.vertices.push(*point);
        self.points_mesh.uvs[0].push(self.color.xy());
        self.points_mesh.uvs[1].push(vec2(self.color.z, self.alpha()));
    }

    /// Emits a camera-facing editor icon centered at `position`.
    pub fn icon(&mut self, icon: GizmoIcon, position: &Vec3, size: f32) {
        let size = self.clamped_icon_world_size(position, size.max(0.0));
        let half = size * 0.5;
        if half <= 0.0 {
            return;
        }

        let right = safe_normalize(self.camera_transform.right(), vec3(1.0, 0.0, 0.0));
        let up = safe_normalize(self.camera_transform.up(), vec3(0.0, 1.0, 0.0));
        let min = *position - right * half - up * half;
        let max_x = *position + right * half - up * half;
        let max = *position + right * half + up * half;
        let min_x = *position - right * half + up * half;
        let [uv_min, uv_max] = icon_uv_rect(icon);
        let color = self.gizmo_color();
        let base = self.icons_mesh.vertices.len() as u32;

        self.icons_mesh.vertices.extend([min, max_x, max, min_x]);
        self.icons_mesh.uvs[0].extend([
            vec2(uv_min.x, uv_max.y),
            uv_max,
            vec2(uv_max.x, uv_min.y),
            uv_min,
        ]);
        self.icons_mesh.uvs[1].extend(std::iter::repeat_n(color.xy(), 4));
        self.icons_mesh.uvs[2].extend(std::iter::repeat_n(vec2(color.z, color.w), 4));
        self.icons_mesh.uvs[3].extend(std::iter::repeat_n(vec2(self.object_id as f32, 0.0), 4));
        self.icons_mesh
            .indices
            .extend([base, base + 1, base + 2, base + 2, base + 3, base]);
    }

    /// Sets the active gizmo color.
    pub fn set_color(&mut self, color: &Vec4) {
        self.color = *color;
    }

    /// Sets the alpha multiplier applied to subsequently emitted gizmos.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }

    /// Enables or disables depth testing for subsequently emitted gizmos.
    pub fn set_depth_test_enabled(&mut self, enabled: bool) {
        self.depth_test_enabled = enabled;
    }

    pub(crate) fn set_object_id(&mut self, object_id: u32) {
        self.object_id = object_id;
    }

    fn gizmo_instance(&self, transform: Mat4, enable_normals: bool) -> GizmoInstance {
        GizmoInstance {
            transform: transform.into(),
            color: *self.gizmo_color().as_ref(),
            enable_normals: enable_normals as i32,
            use_uv_colors: 0,
            _padding: Default::default(),
        }
    }

    fn gizmo_color(&self) -> Vec4 {
        vec4(self.color.x, self.color.y, self.color.z, self.alpha())
    }

    fn alpha(&self) -> f32 {
        self.color.w * self.opacity
    }

    fn clamped_icon_world_size(&self, position: &Vec3, size: f32) -> f32 {
        let depth = glm::dot(
            &(*position - self.camera_transform.position),
            &safe_normalize(self.camera_transform.forward(), vec3(0.0, 0.0, 1.0)),
        );
        let viewport_height = self.viewport_size[1].max(1.0);
        let projection_y = self.camera.projection[(1, 1)].abs();
        if depth <= self.camera.near_plane || projection_y <= f32::EPSILON {
            return size;
        }

        let projected_pixels = size * projection_y * viewport_height / (2.0 * depth);
        if projected_pixels <= f32::EPSILON {
            return size;
        }

        size * projected_pixels.clamp(MIN_ICON_SIZE_PIXELS, MAX_ICON_SIZE_PIXELS) / projected_pixels
    }
}

fn safe_normalize(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.norm_squared() <= f32::EPSILON {
        fallback
    } else {
        glm::normalize(&value)
    }
}

fn icon_uv_rect(icon: GizmoIcon) -> [glm::Vec2; 2] {
    match icon {
        GizmoIcon::Camera => [vec2(0.0, 0.0), vec2(0.5, 1.0)],
        GizmoIcon::Light => [vec2(0.5, 0.0), vec2(1.0, 1.0)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::mesh::Mesh;
    use crate::test_utils::test_asset_context_with_assets;
    use nalgebra_glm::vec3;
    use std::path::PathBuf;

    fn assets_path() -> PathBuf {
        let assets_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets");
        dunce::canonicalize(assets_path).expect("assets dir not found")
    }

    #[test]
    fn icon_size_is_clamped_to_pixel_range() {
        let asset_context = test_asset_context_with_assets(vec![assets_path()]);
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
    fn icon_mesh_stores_object_id_for_picking() {
        let asset_context = test_asset_context_with_assets(vec![assets_path()]);
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
}
