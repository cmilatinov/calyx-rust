use reflect_derive::impl_reflect_value;
use nalgebra_glm as glm;
use glm::{Vec2, Vec3, Vec4, Mat3, Mat4};
use crate as reflect;

impl_reflect_value!(Vec2());
impl_reflect_value!(Vec3());
impl_reflect_value!(Vec4());

impl_reflect_value!(Mat3());
impl_reflect_value!(Mat4());
