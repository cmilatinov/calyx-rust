//! Join snapshot: the server's record of the replicated state changed at
//! runtime, and the snapshot of it the server sends a client that joins.
//!
//! A joining client loads the same scene file as every other peer, so the
//! snapshot carries only what changed since: scene-file objects destroyed,
//! prefabs spawned, ownership, component enabled toggles and the latest synced
//! payload of each synchronized component.

use crate::net::{network_id_range, GameMessage, NetworkObjectId};
use renet::ClientId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Largest encoded size of one [`GameMessage::JoinSnapshot`] chunk the server
/// fills with entries. An entry that alone exceeds it travels in a chunk of
/// its own.
pub const SNAPSHOT_CHUNK_BYTES: usize = 16 * 1024;

/// Room the server leaves on the joining client's reliable ordered channel
/// for live traffic sent while the snapshot is still unacknowledged. The
/// snapshot may use the rest of the channel's memory.
pub const SNAPSHOT_LIVE_HEADROOM_BYTES: usize = 1024 * 1024;

/// One piece of replicated state in a join snapshot.
///
/// Entries come from the server, not from the peer that owns the object, so a
/// client applies them without the ownership checks of the live messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SnapshotEntry {
    /// The object was destroyed: a scene-file object, or one object of a
    /// spawned prefab whose other objects survive.
    Destroy { network_object_id: NetworkObjectId },
    /// A prefab spawned at runtime, with the ids of its network objects and
    /// the peer that spawned it.
    Spawn {
        prefab_id: Uuid,
        network_object_ids: HashMap<u32, NetworkObjectId>,
        owner_id: ClientId,
    },
    /// The current owner of a network object.
    Owner {
        network_object_id: NetworkObjectId,
        owner_id: ClientId,
    },
    /// The enabled state last set on one component of a network object.
    ComponentEnabled {
        network_object_id: NetworkObjectId,
        component_uuid: Uuid,
        enabled: bool,
    },
}

/// A prefab spawn the server accepted.
#[derive(Debug)]
struct SpawnRecord {
    prefab_id: Uuid,
    network_object_ids: HashMap<u32, NetworkObjectId>,
    owner_id: ClientId,
    /// The spawn's ids destroyed since.
    destroyed: BTreeSet<NetworkObjectId>,
}

/// The server's record of every replicated change it accepted at runtime,
/// from which it builds the snapshot a joining client receives.
///
/// The server updates it at the point it relays or applies each change, the
/// host's own changes included, so it holds exactly what every connected
/// peer has seen. It is independent of the host's scene, which may lack the
/// objects of a prefab the host cannot load.
#[derive(Debug, Default)]
pub struct ReplicationRecord {
    /// Scene-file (range 0) objects destroyed at runtime.
    destroyed_scene_objects: BTreeSet<NetworkObjectId>,
    /// Accepted spawns, by the order they were accepted in.
    spawns: BTreeMap<u64, SpawnRecord>,
    next_spawn: u64,
    /// Spawn that created each surviving spawned id.
    spawn_of: HashMap<NetworkObjectId, u64>,
    /// Owner of every spawned object and of every scene-file object whose
    /// ownership was transferred.
    owners: BTreeMap<NetworkObjectId, ClientId>,
    /// Enabled state last set on each component of a network object.
    enabled: BTreeMap<(NetworkObjectId, Uuid), bool>,
    /// Latest synced payload of each synchronized component.
    payloads: BTreeMap<(NetworkObjectId, Uuid), Vec<u8>>,
}

impl ReplicationRecord {
    /// Records a spawn of `prefab_id` by `owner_id` with `network_object_ids`.
    pub fn spawn(
        &mut self,
        owner_id: ClientId,
        prefab_id: Uuid,
        network_object_ids: &HashMap<u32, NetworkObjectId>,
    ) {
        let key = self.next_spawn;
        self.next_spawn += 1;
        for &id in network_object_ids.values() {
            self.spawn_of.insert(id, key);
            self.owners.insert(id, owner_id);
        }
        self.spawns.insert(
            key,
            SpawnRecord {
                prefab_id,
                network_object_ids: network_object_ids.clone(),
                owner_id,
                destroyed: BTreeSet::new(),
            },
        );
    }

    /// Records the destroy of `network_object_id` and forgets its state.
    ///
    /// `scene_object` says whether the host held the object, so that only a
    /// destroy of a real scene-file object is kept, not one of an id no peer
    /// knows. A spawn whose every object is destroyed is dropped.
    pub fn destroy(&mut self, network_object_id: NetworkObjectId, scene_object: bool) {
        self.owners.remove(&network_object_id);
        self.enabled.retain(|(id, _), _| *id != network_object_id);
        self.payloads.retain(|(id, _), _| *id != network_object_id);
        if let Some(key) = self.spawn_of.remove(&network_object_id) {
            if let Some(spawn) = self.spawns.get_mut(&key) {
                spawn.destroyed.insert(network_object_id);
                if spawn.destroyed.len() >= spawn.network_object_ids.len() {
                    self.spawns.remove(&key);
                }
            }
        } else if scene_object && network_id_range(network_object_id) == 0 {
            self.destroyed_scene_objects.insert(network_object_id);
        }
    }

    /// Records the transfer of `network_object_id` to `owner_id`.
    pub fn transfer(&mut self, network_object_id: NetworkObjectId, owner_id: ClientId) {
        self.owners.insert(network_object_id, owner_id);
    }

    /// Records `enabled` as the state of component `component_uuid` of
    /// `network_object_id`.
    pub fn set_component_enabled(
        &mut self,
        network_object_id: NetworkObjectId,
        component_uuid: Uuid,
        enabled: bool,
    ) {
        self.enabled
            .insert((network_object_id, component_uuid), enabled);
    }

    /// Records `data` as the latest synced payload of component
    /// `component_uuid` of `network_object_id`.
    pub fn component_sync(
        &mut self,
        network_object_id: NetworkObjectId,
        component_uuid: Uuid,
        data: &[u8],
    ) {
        match self.payloads.get_mut(&(network_object_id, component_uuid)) {
            Some(payload) => {
                payload.clear();
                payload.extend_from_slice(data);
            }
            None => {
                self.payloads
                    .insert((network_object_id, component_uuid), data.to_vec());
            }
        }
    }

    /// Returns the recorded owner of `network_object_id`: its spawner or the
    /// peer it was last transferred to.
    pub fn owner(&self, network_object_id: NetworkObjectId) -> Option<ClientId> {
        self.owners.get(&network_object_id).copied()
    }

    /// Returns whether `network_object_id` is a surviving object of a
    /// recorded spawn.
    pub fn is_spawned(&self, network_object_id: NetworkObjectId) -> bool {
        self.spawn_of.contains_key(&network_object_id)
    }

    /// Returns the snapshot entries in the order a client applies them:
    /// destroys of scene-file objects, spawns, destroys of spawned objects,
    /// ownership, then component enabled states.
    pub fn entries(&self) -> Vec<SnapshotEntry> {
        let mut entries = Vec::new();
        entries.extend(
            self.destroyed_scene_objects
                .iter()
                .map(|&network_object_id| SnapshotEntry::Destroy { network_object_id }),
        );
        entries.extend(self.spawns.values().map(|spawn| SnapshotEntry::Spawn {
            prefab_id: spawn.prefab_id,
            network_object_ids: spawn.network_object_ids.clone(),
            owner_id: spawn.owner_id,
        }));
        entries.extend(self.spawns.values().flat_map(|spawn| {
            spawn
                .destroyed
                .iter()
                .map(|&network_object_id| SnapshotEntry::Destroy { network_object_id })
        }));
        entries.extend(self.owners.iter().map(|(&network_object_id, &owner_id)| {
            SnapshotEntry::Owner {
                network_object_id,
                owner_id,
            }
        }));
        entries.extend(self.enabled.iter().map(
            |(&(network_object_id, component_uuid), &enabled)| SnapshotEntry::ComponentEnabled {
                network_object_id,
                component_uuid,
                enabled,
            },
        ));
        entries
    }

    /// Builds the snapshot messages for a joining client, in the order they
    /// must be sent: [`GameMessage::JoinSnapshot`] chunks of up to
    /// `chunk_bytes` encoded bytes each, then one
    /// [`GameMessage::SnapshotComponent`] per latest synced payload.
    pub fn messages(&self, chunk_bytes: usize) -> Vec<GameMessage> {
        let mut messages = Vec::new();
        let mut chunk = Vec::new();
        let mut chunk_size = 0;
        for entry in self.entries() {
            let size = encoded_len(&entry);
            if !chunk.is_empty() && chunk_size + size > chunk_bytes {
                messages.push(GameMessage::JoinSnapshot {
                    entries: std::mem::take(&mut chunk),
                });
                chunk_size = 0;
            }
            chunk_size += size;
            chunk.push(entry);
        }
        if !chunk.is_empty() {
            messages.push(GameMessage::JoinSnapshot { entries: chunk });
        }
        messages.extend(self.payloads.iter().map(
            |(&(network_object_id, component_uuid), data)| GameMessage::SnapshotComponent {
                network_object_id,
                component_uuid,
                data: data.clone(),
            },
        ));
        messages
    }
}

/// Encoded size of `value` in the protocol's bincode configuration.
fn encoded_len(value: &impl Serialize) -> usize {
    bincode::serde::encode_to_vec(value, bincode::config::standard())
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

/// Snapshot messages encoded for sending, cut to a byte budget.
#[derive(Debug)]
pub struct SnapshotPlan {
    /// Encoded messages that fit the budget, in send order.
    pub messages: Vec<Vec<u8>>,
    /// Encoded size of every snapshot message, the ones left out included.
    pub total_bytes: usize,
    /// Encoded size of the messages that fit.
    pub sent_bytes: usize,
}

impl SnapshotPlan {
    /// Keeps the longest prefix of `messages` whose encoded size fits in
    /// `budget_bytes`. A prefix keeps the snapshot's order, so whatever is
    /// left out is its tail: the latest payloads first, which owners resend.
    pub fn new(messages: &[GameMessage], budget_bytes: usize) -> Self {
        let encoded: Vec<Vec<u8>> = messages
            .iter()
            .filter_map(|message| {
                bincode::serde::encode_to_vec(message, bincode::config::standard()).ok()
            })
            .collect();
        let total_bytes = encoded.iter().map(Vec::len).sum();
        let mut sent_bytes = 0;
        let mut kept = Vec::with_capacity(encoded.len());
        for bytes in encoded {
            if sent_bytes + bytes.len() > budget_bytes {
                break;
            }
            sent_bytes += bytes.len();
            kept.push(bytes);
        }
        Self {
            messages: kept,
            total_bytes,
            sent_bytes,
        }
    }

    /// Returns whether part of the snapshot did not fit the budget.
    pub fn is_truncated(&self) -> bool {
        self.sent_bytes < self.total_bytes
    }
}
