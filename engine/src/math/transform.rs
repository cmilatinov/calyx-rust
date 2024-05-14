use glm::{DQuat, DVec3, Mat4, Quat, Vec3};
use nalgebra::{Matrix4, Quaternion, UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};

use crate as engine;
use crate::reflect::Reflect;

use crate::utils::TypeUuid;

use super::{compose_transform, decompose_transform};

#[derive(Copy, Clone, Debug, Serialize, Deserialize, TypeUuid, Reflect)]
#[serde(from = "TransformShadow")]
pub struct Transform {
    #[serde(skip)]
    pub position: Vector3<f32>,
    #[serde(skip)]
    pub rotation: Vector3<f32>,
    #[serde(skip)]
    pub scale: Vector3<f32>,
    pub matrix: Matrix4<f32>,
    #[serde(skip)]
    pub inverse_matrix: Matrix4<f32>,
}

impl Default for Transform {
    fn default() -> Self {
        let position = Vec3::default();
        let rotation = Vec3::default();
        let scale = glm::vec3(1.0, 1.0, 1.0);
        let matrix = compose_transform(&position, &rotation, &scale);
        let inverse_matrix = glm::inverse(&matrix);
        Transform {
            position,
            rotation,
            scale,
            matrix,
            inverse_matrix,
        }
    }
}

impl From<Mat4> for Transform {
    fn from(matrix: Mat4) -> Self {
        let inverse_matrix = glm::inverse(&matrix);
        let mut transform = Transform {
            matrix,
            inverse_matrix,
            ..Default::default()
        };
        transform.update_components();
        transform
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
        value.matrix
    }
}

impl Transform {
    pub fn from_components(position: Vec3, rotation: Vec3, scale: Vec3) -> Self {
        let mut transform = Transform {
            position,
            rotation,
            scale,
            matrix: Mat4::default(),
            inverse_matrix: Mat4::default(),
        };
        transform.update_matrix();
        transform
    }

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        let mut transform = Transform {
            position: Vec3::new(x, y, z),
            rotation: Vec3::zeros(),
            scale: Vec3::new(1.0, 1.0, 1.0),
            matrix: Default::default(),
            inverse_matrix: Default::default(),
        };
        transform.update_matrix();
        transform
    }

    pub fn look_at(&mut self, position: &Vec3) {
        let diff = self.position - position;
        if glm::length(&diff) <= 0.000001f32 {
            return;
        }
        let rotation = glm::quat_look_at(&glm::normalize(&diff), &glm::vec3(0f32, 1f32, 0f32));
        self.rotation = glm::quat_euler_angles(&rotation);
        self.update_matrix();
    }

    pub fn transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed = self.matrix * glm::vec4(position.x, position.y, position.z, 1.0);
        glm::vec3(transformed.x, transformed.y, transformed.z)
    }

    pub fn transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.matrix);
        matrix * direction
    }

    pub fn inverse_transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed =
            self.get_inverse_matrix() * glm::vec4(position.x, position.y, position.z, 1.0);
        glm::vec3(transformed.x, transformed.y, transformed.z)
    }

    pub fn inverse_transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.get_inverse_matrix());
        matrix * direction
    }

    pub fn set_local_matrix(&mut self, matrix: &Mat4) {
        self.matrix = *matrix;
        self.update_components();
    }

    pub fn translate(&mut self, translation: &Vec3) {
        self.position += translation;
        self.update_matrix();
    }

    pub fn rotate(&mut self, rotation: &Vec3) {
        self.rotation += rotation;
        self.update_matrix();
    }

    pub fn scale(&mut self, scale: &Vec3) {
        self.scale.x *= scale.x;
        self.scale.y *= scale.y;
        self.scale.z *= scale.z;
        self.update_matrix();
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

    pub fn update_matrix(&mut self) {
        self.matrix = compose_transform(&self.position, &self.rotation, &self.scale);
        self.inverse_matrix = glm::inverse(&self.matrix);
    }

    pub fn update_components(&mut self) {
        decompose_transform(
            &self.matrix,
            &mut self.position,
            &mut self.rotation,
            &mut self.scale,
        );
    }

    pub fn get_inverse_matrix(&self) -> Mat4 {
        glm::inverse(&self.matrix)
    }
}

#[derive(Deserialize)]
struct TransformShadow {
    matrix: Mat4,
}

impl From<TransformShadow> for Transform {
    fn from(value: TransformShadow) -> Self {
        let TransformShadow { matrix } = value;
        let mut transform = Transform {
            inverse_matrix: glm::inverse(&matrix),
            matrix,
            ..Default::default()
        };
        transform.update_components();
        transform
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
            glm::quat_euler_angles(&Quaternion::new(
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
            rotation: nalgebra::convert::<Quat, DQuat>(
                *UnitQuaternion::from_euler_angles(
                    value.rotation.x,
                    value.rotation.y,
                    value.rotation.z,
                )
                .quaternion(),
            )
            .into(),
            translation: nalgebra::convert::<Vec3, DVec3>(value.position).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::math::transform::Transform;

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
