//! Multiplayer and replication APIs.
//!
//! The net module exposes the high-level client, server, message, and network
//! ownership types used to drive the engine's Renet-based multiplayer layer.

mod client;
mod component;
mod ids;
mod message;
mod network;
mod server;
mod snapshot;
mod sync;

pub use client::*;
pub use component::*;
pub use ids::*;
pub use message::*;
pub use network::*;
pub use server::*;
pub use snapshot::*;

#[cfg(test)]
mod tests;
