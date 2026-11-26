use nalgebra::{Quaternion, Unit, UnitQuaternion};
use nalgebra_glm as glm;
use nalgebra_glm::{DQuat, DVec3, Mat4, Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate as calyx_engine;
use crate::reflect::Reflect;

use crate::utils::TypeUuid;

use super::{compose_transform, decompose_transform};

/// Position, rotation, and scale stored as decomposed transform components.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, TypeUuid, Reflect)]
pub struct Transform {
    /// Local-space translation.
    pub position: Vec3,
    /// Local-space orientation.
    pub rotation: UnitQuaternion<f32>,
    /// Local-space non-uniform scale.
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
    /// Builds a transform from explicit components.
    pub fn from_components(position: Vec3, rotation: UnitQuaternion<f32>, scale: Vec3) -> Self {
        Transform {
            position,
            rotation,
            scale,
        }
    }

    /// Builds a transform with identity rotation and unit scale.
    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Transform {
            position: Vec3::new(x, y, z),
            rotation: UnitQuaternion::identity(),
            scale: Vec3::new(1.0, 1.0, 1.0),
        }
    }

    /// Rotates this transform to face `position`.
    pub fn look_at(&mut self, position: &Vec3) {
        let diff = self.position - position;
        if glm::length(&diff) <= 0.000001f32 {
            return;
        }
        self.rotation = UnitQuaternion::look_at_rh(&diff.normalize(), &Vec3::y_axis());
    }

    /// Transforms a point from local space into parent space.
    pub fn transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed = self.matrix() * glm::vec4(position.x, position.y, position.z, 1.0);
        glm::vec3(transformed.x, transformed.y, transformed.z)
    }

    /// Transforms a direction from local space into parent space.
    pub fn transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.matrix());
        matrix * direction
    }

    /// Transforms a point from parent space into local space.
    pub fn inverse_transform_position(&self, position: &Vec3) -> Vec3 {
        let transformed =
            self.inverse_matrix() * Vec4::new(position.x, position.y, position.z, 1.0);
        Vec3::new(transformed.x, transformed.y, transformed.z)
    }

    /// Transforms a direction from parent space into local space.
    pub fn inverse_transform_direction(&self, direction: &Vec3) -> Vec3 {
        let matrix = glm::mat4_to_mat3(&self.inverse_matrix());
        matrix * direction
    }

    /// Replaces this transform with the decomposition of `matrix`.
    pub fn set_local_matrix(&mut self, matrix: &Mat4) {
        *self = matrix.into();
    }

    /// Adds `translation` to the current position.
    pub fn translate(&mut self, translation: &Vec3) {
        self.position += translation;
    }

    /// Post-multiplies the current rotation by `rotation`.
    pub fn rotate(&mut self, rotation: &UnitQuaternion<f32>) {
        self.rotation *= rotation;
    }

    /// Multiplies the current scale component-wise by `scale`.
    pub fn scale(&mut self, scale: &Vec3) {
        self.scale = self.scale.component_mul(scale);
    }

    /// Returns the local forward axis in parent space.
    pub fn forward(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(0.0, 0.0, 1.0))
    }

    /// Returns the local right axis in parent space.
    pub fn right(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(1.0, 0.0, 0.0))
    }

    /// Returns the local up axis in parent space.
    pub fn up(&self) -> Vec3 {
        self.transform_direction(&glm::vec3(0.0, 1.0, 0.0))
    }

    /// Returns the composed transform matrix.
    pub fn matrix(&self) -> Mat4 {
        compose_transform(&self.position, &self.rotation, &self.scale)
    }

    /// Returns the inverse of the composed transform matrix.
    pub fn inverse_matrix(&self) -> Mat4 {
        let inv_scale = Vec3::new(1.0 / self.scale.x, 1.0 / self.scale.y, 1.0 / self.scale.z);
        let inv_rot = self.rotation.conjugate();
        glm::scaling(&inv_scale) * glm::quat_to_mat4(&inv_rot) * glm::translation(&-self.position)
    }

    /// Blends weighted transforms using normalized linear interpolation.
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
        if (total_weight - 1.0).abs() > f32::EPSILON && total_weight > 0.0 {
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
mod tests;
