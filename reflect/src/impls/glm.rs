use glm::{Mat3, Mat4, Vec2, Vec3, Vec4};
use nalgebra_glm as glm;

use reflect_derive::{impl_extern_type_uuid, impl_reflect_value};

use crate as reflect;
use crate::ReflectDefault;

impl_extern_type_uuid!(Vec2, "c16b091f-edfa-46d5-8316-5eab5550fa34");
impl_reflect_value!(Vec2(Default));
impl_extern_type_uuid!(Vec3, "6e70688c-98e9-4f09-9869-90be09c25f88");
impl_reflect_value!(Vec3(Default));
impl_extern_type_uuid!(Vec4, "1ddc4442-5d57-4234-856b-26f6b2179c14");
impl_reflect_value!(Vec4(Default));

impl_extern_type_uuid!(Mat3, "f7686e07-1e79-4fd4-a407-1477ddc7f541");
impl_reflect_value!(Mat3(Default));
impl_extern_type_uuid!(Mat4, "2e1650e5-4718-40a9-9f82-fbe8d8048727");
impl_reflect_value!(Mat4(Default));
