use crate::net::NetworkObjectId;

/// Range of network object ids the server reserves for one peer.
///
/// A network object id is `range << 32 | counter`. The server hands each peer
/// its own range when it joins (the host takes the first one) and never gives
/// a range out twice in a session, so ids chosen by different peers never
/// collide. Range 0 is never assigned: it is left to network objects authored
/// into a scene.
pub type NetworkIdRange = u32;

/// Returns the range network object `network_object_id` belongs to.
pub fn network_id_range(network_object_id: NetworkObjectId) -> NetworkIdRange {
    (network_object_id >> 32) as NetworkIdRange
}

/// Allocates network object ids for the local peer from the range the server
/// assigned it.
#[derive(Debug, Default)]
pub struct NetworkIdAllocator {
    range: Option<NetworkIdRange>,
    next: u32,
}

impl NetworkIdAllocator {
    /// Allocates from `range` from now on, restarting the counter when the
    /// range changes.
    pub fn assign(&mut self, range: NetworkIdRange) {
        if self.range != Some(range) {
            self.range = Some(range);
            self.next = 1;
        }
    }

    /// Returns the range ids are allocated from, once the server assigned one.
    pub fn range(&self) -> Option<NetworkIdRange> {
        self.range
    }

    /// Returns the next unused id, or `None` before the server assigned a
    /// range or once the range is used up.
    pub fn allocate(&mut self) -> Option<NetworkObjectId> {
        let range = self.range?;
        if self.next == u32::MAX {
            return None;
        }
        let id = (NetworkObjectId::from(range) << 32) | NetworkObjectId::from(self.next);
        self.next += 1;
        Some(id)
    }
}
