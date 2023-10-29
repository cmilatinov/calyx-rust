use glm::{Mat3, Mat4, Vec2, Vec3, Vec4};
use nalgebra_glm as glm;

use reflect_derive::impl_reflect_value;

use crate as reflect;

impl_reflect_value!(Vec2());
impl_reflect_value!(Vec3());
impl_reflect_value!(Vec4());

impl_reflect_value!(Mat3());
impl_reflect_value!(Mat4());
