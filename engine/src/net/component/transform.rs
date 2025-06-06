use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "9eb02caf-dcf2-4ea4-98bf-5c170230b9a2"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Transform")]
#[serde(default)]
#[repr(C)]
pub struct NetworkTransform {}

impl Component for NetworkTransform {}
