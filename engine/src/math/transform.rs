use nalgebra::{Quaternion, Unit, UnitQuaternion};
use nalgebra_glm as glm;
use nalgebra_glm::{DQuat, DVec3, Mat3, Mat4, Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::reflect::Reflect;

use crate::utils::TypeUuid;

use super::{compose_transform, decompose_transform};

#[derive(Copy, Clone, Debug, Serialize, Deserialize, TypeUuid, Reflect)]
#[repr(C)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: UnitQuaternion<f32>,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        let position = Vec3::default();
        let rotation = UnitQuaternion::identity();
        let scale = Vec3::from_element(1.0);
        Transform {
            position,
            rotation,
            scale,
        }
    }
}

impl From<&Mat4> for Transform {
    fn from(matrix: &Mat4) -> Self {
        let mut transform = Transform {
            ..Default::default()
        };
        decompose_transform(
            matrix,
            &mut transform.position,
            &mut transform.rotation,
            &mut transform.scale,
        );
        transform
    }
}

impl From<Mat4> for Transform {
    fn from(matrix: Mat4) -> Self {
        (&matrix).into()
    }
}

impl From<mint::ColumnMatrix4<f32>> for Transform {
    fn from(matrix: mint::ColumnMatrix4<f32>) -> Self {
        let matrix: Mat4 = matrix.into();
        matrix.into()
    }
}

impl From<Transform> for Mat4 {
    fn from(value: Transform) -> Self {
        value.matrix()
    }
}

impl Transform {
    pub fn from_components(position: Vec3, rotation: UnitQuaternion<f32>, scale: Vec3) -> Self {
        Transform {
            position,
            rotation,
            scale,
        }
    }

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Transform {
            position: Vec3::new(x, y, z),
            rotation: UnitQuaternion::identity(),
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    pub fn look_at(&mut self, position: &Vec3) {
        let diff = self.position - position;
        if glm::length(&diff) <= 0.000001f32 {
            return;
        }
        self.rotation = UnitQuaternion::look_at_rh(&diff.normalize(), &Vec3::y_axis());
    }

    pub fn transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed = self.matrix() * glm::vec4(position.x, position.y, position.z, 1.0);
        glm::vec3(transformed.x, transformed.y, transformed.z)
    }

    pub fn transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.matrix());
        matrix * direction
    }

    pub fn inverse_transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed =
            self.inverse_matrix() * Vec4::new(position.x, position.y, position.z, 1.0);
        Vec3::new(transformed.x, transformed.y, transformed.z)
    }

    pub fn inverse_transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.inverse_matrix());
        matrix * direction
    }

    pub fn set_local_matrix(&mut self, matrix: &Mat4) {
        *self = matrix.into();
    }

    pub fn translate(&mut self, translation: &Vec3) {
        self.position += translation;
    }

    pub fn rotate(&mut self, rotation: &UnitQuaternion<f32>) {
        self.rotation *= rotation;
    }

    pub fn scale(&mut self, scale: &Vec3) {
        self.scale = self.scale.component_mul(scale);
    }

    pub fn forward(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(0.0, 0.0, 1.0))
    }

    pub fn right(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(1.0, 0.0, 0.0))
    }

    pub fn up(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(0.0, 1.0, 0.0))
    }

    pub fn matrix(&self) -> Mat4 {
        compose_transform(&self.position, &self.rotation, &self.scale)
    }

    pub fn inverse_matrix(&self) -> Mat4 {
        let inv_scale = Vec3::new(1.0 / self.scale.x, 1.0 / self.scale.y, 1.0 / self.scale.z);
        let inv_rot = self.rotation.conjugate();

        // Build inverse matrix directly
        let rot_mat = inv_rot.to_rotation_matrix();
        let scaled_rot = rot_mat.matrix() * Mat3::from_diagonal(&inv_scale);

        // Apply inverse translation
        let inv_translation = -(scaled_rot * self.position);

        Mat4::new(
            scaled_rot[(0, 0)],
            scaled_rot[(0, 1)],
            scaled_rot[(0, 2)],
            inv_translation.x,
            scaled_rot[(1, 0)],
            scaled_rot[(1, 1)],
            scaled_rot[(1, 2)],
            inv_translation.y,
            scaled_rot[(2, 0)],
            scaled_rot[(2, 1)],
            scaled_rot[(2, 2)],
            inv_translation.z,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn nlerp(transforms: impl Iterator<Item = (f32, Transform)>) -> Transform {
        let mut position = Vec3::zeros();
        let mut rotation = Quat::new(0.0, 0.0, 0.0, 0.0);
        let mut scale = Vec3::new(1.0, 1.0, 1.0);
        let mut total_weight = 0.0;

        for (weight, transform) in transforms {
            total_weight += weight;
            position += transform.position * weight;

            // Simpler scaling approach - direct linear blend
            // This works reasonably well for moderate scaling differences
            scale += (transform.scale - Vec3::new(1.0, 1.0, 1.0)) * weight;

            let q = transform.rotation.into_inner();
            let dot = rotation.dot(&q);
            let corrected_q = if dot < 0.0 { -q } else { q };
            rotation += corrected_q * weight;
        }

        // Normalize if needed
        if (total_weight - 1.0).abs() > std::f32::EPSILON && total_weight > 0.0 {
            position /= total_weight;
            scale = Vec3::new(1.0, 1.0, 1.0) + (scale - Vec3::new(1.0, 1.0, 1.0)) / total_weight;
        }

        let rotation = Unit::new_normalize(rotation);

        Transform {
            position,
            rotation,
            scale,
        }
    }
}

impl From<transform_gizmo_egui::math::Transform> for Transform {
    fn from(value: transform_gizmo_egui::math::Transform) -> Self {
        Transform::from_components(
            Vec3::new(
                value.translation.x as f32,
                value.translation.y as f32,
                value.translation.z as f32,
            ),
            UnitQuaternion::new_normalize(Quaternion::new(
                value.rotation.s as f32,
                value.rotation.v.x as f32,
                value.rotation.v.y as f32,
                value.rotation.v.z as f32,
            )),
            Vec3::new(
                value.scale.x as f32,
                value.scale.y as f32,
                value.scale.z as f32,
            ),
        )
    }
}

impl From<Transform> for transform_gizmo_egui::math::Transform {
    fn from(value: Transform) -> Self {
        Self {
            scale: nalgebra::convert::<Vec3, DVec3>(value.scale).into(),
            rotation: nalgebra::convert::<Quat, DQuat>(*value.rotation.quaternion()).into(),
            translation: nalgebra::convert::<Vec3, DVec3>(value.position).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::math::transform::Transform;
    use nalgebra_glm as glm;

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
}
