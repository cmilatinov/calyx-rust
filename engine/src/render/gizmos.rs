use std::iter;

use glm::{vec2, vec3, Mat4, Vec3, Vec4};

use crate::assets::mesh::Mesh;
use crate::math::{compose_transform, Transform};
use crate::render::{Camera, GizmoInstance};

pub struct Gizmos<'a> {
    pub(crate) camera_transform: &'a Transform,
    pub(crate) color: Vec4,
    pub(crate) circle_list: &'a mut Vec<GizmoInstance>,
    pub(crate) cube_list: &'a mut Vec<GizmoInstance>,
    pub(crate) wire_circle_mesh: &'a mut Mesh,
    pub(crate) wire_cube_mesh: &'a mut Mesh,
    pub(crate) lines_mesh: &'a mut Mesh,
    pub(crate) points_mesh: &'a mut Mesh,
}

impl<'a> Gizmos<'a> {
    pub fn wire_sphere(&mut self, center: &Vec3, radius: f32) {
        let instances = &mut self.wire_circle_mesh.instances;
        let translation = glm::translate(&Mat4::identity(), center);
        let scale = vec3(radius, radius, radius);
        instances.push(glm::scale(&translation, &scale).into());
        instances
            .push(glm::scale(&glm::rotate_x(&translation, 90.0f32.to_radians()), &scale).into());
        instances
            .push(glm::scale(&glm::rotate_y(&translation, 90.0f32.to_radians()), &scale).into());

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
        instances.push(t.into());
        self.circle_list
            .extend(iter::repeat(self.gizmo_instance(true)).take(3));
        self.circle_list.push(self.gizmo_instance(false));
    }

    pub fn wire_cube(&mut self, position: &Vec3, size: &Vec3) {
        self.wire_cube_mesh
            .instances
            .push(compose_transform(position, &vec3(0.0, 0.0, 0.0), size).into());
        self.cube_list.push(self.gizmo_instance(false));
    }

    pub fn wire_frustum(
        &mut self,
        transform: &Transform,
        aspect: f32,
        fov: f32,
        near_plane: f32,
        far_plane: f32,
    ) {
        let camera = Camera::new(aspect, fov, near_plane, far_plane);
        let matrix = glm::inverse(&(camera.projection * transform.inverse_matrix));
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

    pub fn line(&mut self, start: &Vec3, end: &Vec3) {
        self.lines_mesh.vertices.push(*start);
        self.lines_mesh.vertices.push(*end);
        self.lines_mesh.uvs[0].extend(iter::repeat(self.color.xy()).take(2));
        self.lines_mesh.uvs[1].extend(iter::repeat(vec2(self.color.z, self.color.w)).take(2));
    }

    pub fn point(&mut self, point: &Vec3) {
        self.points_mesh.vertices.push(*point);
        self.points_mesh.uvs[0].push(self.color.xy());
        self.points_mesh.uvs[1].push(vec2(self.color.z, self.color.w));
    }

    pub fn set_color(&mut self, color: &Vec4) {
        self.color = *color;
    }

    fn gizmo_instance(&self, enable_normals: bool) -> GizmoInstance {
        GizmoInstance {
            color: *self.color.as_ref(),
            enable_normals: enable_normals as i32,
            use_uv_colors: 0,
        }
    }
}
