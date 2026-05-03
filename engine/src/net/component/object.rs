use crate as engine;
use crate::component::{Component, ReflectComponent};
use crate::net::Network;
use crate::reflect::{Reflect, ReflectDefault};
use crate::utils::{ReflectTypeUuidDynamic, TypeUuid};
use renet::ClientId;
use serde::{Deserialize, Serialize};

/// Stable identifier assigned to a networked game object.
pub type NetworkObjectId = u64;

/// Component that marks a game object as replicated and assigns an owning peer.
#[derive(Default, TypeUuid, Serialize, Deserialize, Component, Reflect)]
#[uuid = "adca3c70-5d3d-4b32-83cc-7bdf04a8358a"]
#[reflect(Default, TypeUuidDynamic, Component)]
#[reflect_attr(name = "Network Object")]
#[serde(default)]
#[repr(C)]
pub struct ComponentNetworkObject {
    /// Replication identifier shared across peers.
    pub id: NetworkObjectId,
    /// Client that currently owns authoritative updates for this object.
    pub owner_id: ClientId,
}

impl Component for ComponentNetworkObject {}

impl ComponentNetworkObject {
    /// Returns whether the local peer currently owns this object.
    pub fn is_owner(&self, network: &Network) -> bool {
        network.local_id == Some(self.owner_id)
    }
}
