use std::cell::RefCell;
use super::{compose_transform, decompose_transform};
use glm::{Vec3, Mat4};

pub struct Transform {
    parent: Option<RefCell<Box<Transform>>>,

    position: Vec3,
    rotation: Vec3,
    scale: Vec3,

    matrix: Mat4
}

impl Default for Transform {
    fn default() -> Self {
        let position = Vec3::new(0f32, 0f32, 0f32);
        let rotation= Vec3::new(0f32, 0f32, 0f32);
        let scale = Vec3::new(1f32, 1f32, 1f32);
        let matrix = compose_transform(&position, &rotation, &scale);

        Transform {
            parent: None,

            position,
            rotation,
            scale,

            matrix
        }
    }
}

impl Transform {
    pub fn look_at(&mut self, position: &Vec3) {
        let diff = self.position - position;
        if glm::length(&diff) <= 0.000001f32 {
            return;
        }
        let rotation = glm::quat_look_at(&glm::normalize(&diff), &glm::vec3(0f32,1f32,0f32));
        self.rotation = glm::quat_euler_angles(&rotation);
        self.update_matrix();
    }

    pub fn get_inverse_matrix(&mut self) -> Mat4 {
        return match &self.parent {
            None => {
                glm::inverse(&self.matrix)
            }
            Some(p) => {
                glm::inverse(&(self.matrix * p.borrow_mut().get_inverse_matrix()))
            }
        }
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
        let transformed = glm::inverse(&self.matrix) * glm::vec4(position.x, position.y, position.z, 1.0);
        glm::vec3(transformed.x, transformed.y, transformed.z)
    }

    pub fn inverse_transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&glm::inverse(&self.matrix));
        matrix * direction
    }

    pub fn set_world_matrix(&mut self, matrix: &Mat4) {
        match &self.parent {
            None => {
                self.set_local_matrix(matrix);
            }
            Some(p) => {
                self.matrix = p.borrow_mut().get_inverse_matrix() * matrix;
                self.update_components();
            }
        }
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

    pub fn forward(&self) {
        self.transform_direction(&glm::vec3(0f32, 0f32, 1f32));
    }

    pub fn right(&self)  {
        self.transform_direction(&glm::vec3(1f32, 0f32, 0f32));
    }

    pub fn up(&self)  {
        self.transform_direction(&glm::vec3(0f32, 1f32, 0f32));
    }

    pub fn update_matrix(&mut self) {
        self.matrix = compose_transform(&self.position, &self.rotation, &self.scale);
    }

    pub fn update_components(&mut self) {
        decompose_transform(&self.matrix, &mut self.position, &mut self.rotation, &mut self.scale);
    }

    pub fn get_position(&self) -> Vec3 {
        self.position
    }

    pub fn get_rotation(&self) -> Vec3 {
        self.rotation
    }

    pub fn get_scale(&self) -> Vec3 {
        self.scale
    }

    pub fn get_matrix(&self) -> Mat4 {
        return match &self.parent {
            None => {
                self.matrix
            }
            Some(p) => {
                p.borrow().matrix * self.matrix
            }
        }
    }

    pub fn get_parent_matrix(&self) -> Option<Mat4> {
        return match &self.parent {
            None => {None}
            Some(p) => {
                Some(p.borrow().matrix)
            }
        }
    }
}