use engine_derive::impl_reflect_value;
use rapier3d::dynamics::{RigidBodyHandle, RigidBodyType};

use crate as engine;
use crate::reflect::ReflectDefault;

impl_reflect_value!(RigidBodyType());
impl_reflect_value!(RigidBodyHandle(Default));
