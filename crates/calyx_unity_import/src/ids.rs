//! Name-based (version 5) UUIDs, as RFC 4122 defines them, so the ids the
//! importer derives from asset and object names stay the same across runs.

use sha1::{Digest, Sha1};
use uuid::Uuid;

/// The version 5 UUID of `name` in `namespace`: the first 16 bytes of the
/// SHA-1 of the namespace's bytes followed by the name, with the version and
/// variant bits set.
pub fn uuid_v5(namespace: &Uuid, name: &[u8]) -> Uuid {
    let mut hasher = Sha1::new();
    hasher.update(namespace.as_bytes());
    hasher.update(name);
    let hash = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests;
