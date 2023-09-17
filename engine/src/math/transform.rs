use glm::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use reflect::Reflect;

use super::{compose_transform, decompose_transform};

#[derive(Copy, Clone, Serialize, Deserialize, Reflect)]
pub struct Transform {
    #[serde(skip)]
    pub position: Vec3,
    #[serde(skip)]
    pub rotation: Vec3,
    #[serde(skip)]
    pub scale: Vec3,
    pub matrix: Mat4,
    #[serde(skip)]
    pub inverse_matrix: Mat4,
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
            inverse_matrix
        }
    }
}

impl Transform {
    pub fn from_matrix(matrix: Mat4) -> Self {
        let inverse_matrix = glm::inverse(&matrix);
        let mut transform = Transform {
            position: Vec3::default(),
            rotation: Vec3::default(),
            scale: Vec3::identity(),
            matrix,
            inverse_matrix
        };
        transform.update_components();
        transform
    }

    pub fn from_components(position: Vec3, rotation: Vec3, scale: Vec3) -> Self {
        let mut transform = Transform {
            position,
            rotation,
            scale,
            matrix: Mat4::default(),
            inverse_matrix: Mat4::default()
        };
        transform.update_matrix();
        transform
    }

    pub fn look_at(&mut self, position: &Vec3) {
        let diff = self.position - position;
        if glm::length(&diff) <= 0.000001f32 {
            return;
        }
        let rotation = glm::quat_look_at(&glm::normalize(&diff), &glm::vec3(0f32,1f32,0f32));
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
        let transformed = self.get_inverse_matrix() * glm::vec4(position.x, position.y, position.z, 1.0);
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

    pub fn right(&self) -> Vec3  {
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
        decompose_transform(&self.matrix, &mut self.position, &mut self.rotation, &mut self.scale);
    }

    pub fn get_inverse_matrix(&self) -> Mat4 { glm::inverse(&self.matrix) }
}

#[cfg(test)]
mod tests {
    use crate::math::transform::Transform;

    #[test]
    fn basic_transform_translation() {
        let transform = Transform::default();

        assert_eq!(transform.forward(), glm::vec3(0f32, 0f32, 1f32));
        assert_eq!(transform.forward().scale(-1f32), glm::vec3(0f32, 0f32, -1f32));

        assert_eq!(transform.right(), glm::vec3(1f32, 0f32, 0f32));
        assert_eq!(transform.right().scale(-1f32), glm::vec3(-1f32, 0f32, 0f32));

        assert_eq!(transform.up(), glm::vec3(0f32, 1f32, 0f32));
        assert_eq!(transform.up().scale(-1f32), glm::vec3(0f32, -1f32, 0f32));
    }
}
