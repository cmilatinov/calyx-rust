use engine_derive::impl_reflect_value;
use nalgebra_glm::{Mat3, Mat4, Vec2, Vec3, Vec4};

use crate as engine;
use crate::reflect::ReflectDefault;

impl_reflect_value!(Vec2(Default));
impl_reflect_value!(Vec3(Default));
impl_reflect_value!(Vec4(Default));
impl_reflect_value!(Mat3(Default));
impl_reflect_value!(Mat4(Default));
