use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::net::NetworkId;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use serde::{Deserialize, Serialize};

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "adca3c70-5d3d-4b32-83cc-7bdf04a8358a"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Object")]
#[serde(default)]
#[repr(C)]
pub struct NetworkObject {
    pub id: NetworkId,
    pub is_owner: bool,
}

impl Component for NetworkObject {}
