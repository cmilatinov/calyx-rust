use engine_derive::{impl_extern_type_uuid, impl_reflect_value};
use rapier3d::dynamics::{RigidBodyHandle, RigidBodyType};

use crate as engine;
use crate::reflect::ReflectDefault;

impl_extern_type_uuid!(RigidBodyType, "187b0475-fcfa-4c36-88a6-c2d35510fd51");
impl_extern_type_uuid!(RigidBodyHandle, "3a3f95d5-823c-4468-9cd9-577ae25d5f6d");

impl_reflect_value!(RigidBodyType());
impl_reflect_value!(RigidBodyHandle(Default));
