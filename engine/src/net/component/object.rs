use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::net::Network;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use renet::ClientId;
use serde::{Deserialize, Serialize};

pub type NetworkObjectId = u32;

#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "adca3c70-5d3d-4b32-83cc-7bdf04a8358a"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Object")]
#[serde(default)]
#[repr(C)]
pub struct ComponentNetworkObject {
    pub id: NetworkObjectId,
    pub owner_id: ClientId,
}

impl Component for ComponentNetworkObject {}

impl ComponentNetworkObject {
    pub fn is_owner(&self, network: &Network) -> bool {
        network.local_id == Some(self.owner_id)
    }
}
