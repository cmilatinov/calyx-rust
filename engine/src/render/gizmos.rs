use std::iter;

use crate::assets::mesh::Mesh;
use crate::math::{compose_transform, Transform};
use crate::render::{Camera, GizmoInstance};
use nalgebra::UnitQuaternion;
use nalgebra_glm as glm;
use nalgebra_glm::{vec2, vec3, vec4, Mat4, Vec3, Vec4};

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
    pub(crate) camera_transform: &'a Transform,
    pub(crate) color: Vec4,
    pub(crate) opacity: f32,
    pub(crate) depth_test_enabled: bool,
    pub(crate) circle_list: &'a mut Vec<GizmoInstance>,
    pub(crate) cube_list: &'a mut Vec<GizmoInstance>,
    pub(crate) lines_mesh: &'a mut Mesh,
    pub(crate) points_mesh: &'a mut Mesh,
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
        self.lines_mesh.uvs[0].extend(iter::repeat(self.color.xy()).take(2));
        self.lines_mesh.uvs[1].extend(iter::repeat(vec2(self.color.z, self.alpha())).take(2));
    }

    /// Emits a colored point.
    pub fn point(&mut self, point: &Vec3) {
        self.points_mesh.vertices.push(*point);
        self.points_mesh.uvs[0].push(self.color.xy());
        self.points_mesh.uvs[1].push(vec2(self.color.z, self.alpha()));
    }

    /// Emits a camera-facing editor icon centered at `position`.
    pub fn icon(&mut self, icon: GizmoIcon, position: &Vec3, size: f32) {
        let half = size.max(0.0) * 0.5;
        if half <= 0.0 {
            return;
        }

        let right = safe_normalize(self.camera_transform.right(), vec3(1.0, 0.0, 0.0));
        let up = safe_normalize(self.camera_transform.up(), vec3(0.0, 1.0, 0.0));
        match icon {
            GizmoIcon::Camera => {
                let left = *position - right * half;
                let right_edge = *position + right * half;
                let top = *position + up * half;
                let bottom = *position - up * half;
                let lens = *position + right * (half * 1.35);
                self.line(&left, &top);
                self.line(&top, &right_edge);
                self.line(&right_edge, &bottom);
                self.line(&bottom, &left);
                self.line(&(right_edge + up * (half * 0.35)), &lens);
                self.line(&lens, &(right_edge - up * (half * 0.35)));
            }
            GizmoIcon::Light => {
                let radius = half * 0.55;
                for i in 0..8 {
                    let a = (i as f32) * std::f32::consts::TAU / 8.0;
                    let b = ((i + 1) as f32) * std::f32::consts::TAU / 8.0;
                    let start = *position + right * (a.cos() * radius) + up * (a.sin() * radius);
                    let end = *position + right * (b.cos() * radius) + up * (b.sin() * radius);
                    self.line(&start, &end);
                }
                self.line(&(*position - right * half), &(*position + right * half));
                self.line(&(*position - up * half), &(*position + up * half));
            }
        }
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
}

fn safe_normalize(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.norm_squared() <= f32::EPSILON {
        fallback
    } else {
        glm::normalize(&value)
    }
}
